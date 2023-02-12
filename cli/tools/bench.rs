// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use crate::args::BenchOptions;
use crate::args::CliOptions;
use crate::args::TypeCheckMode;
use crate::colors;
use crate::display::write_json_to_stdout;
use crate::graph_util::graph_valid_with_cli_options;
use crate::ops;
use crate::proc_state::ProcState;
use crate::tools::test::format_test_error;
use crate::tools::test::TestFilter;
use crate::util::file_watcher;
use crate::util::file_watcher::ResolutionResult;
use crate::util::fs::collect_specifiers;
use crate::util::path::is_supported_ext;
use crate::version::get_user_agent;
use crate::worker::create_main_worker_for_test_or_bench;

use deno_core::error::generic_error;
use deno_core::error::AnyError;
use deno_core::error::JsError;
use deno_core::futures::future;
use deno_core::futures::stream;
use deno_core::futures::FutureExt;
use deno_core::futures::StreamExt;
use deno_core::ModuleSpecifier;
use deno_runtime::permissions::Permissions;
use deno_runtime::permissions::PermissionsContainer;
use deno_runtime::tokio_util::run_local;
use indexmap::IndexMap;
use log::Level;
use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
struct BenchSpecifierOptions {
  filter: TestFilter,
  json: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchPlan {
  pub total: usize,
  pub origin: String,
  pub used_only: bool,
  pub names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BenchEvent {
  Plan(BenchPlan),
  Output(String),
  Register(BenchDescription),
  Wait(usize),
  Result(usize, BenchResult),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BenchResult {
  Ok(BenchStats),
  Failed(Box<JsError>),
}

#[derive(Debug, Clone)]
pub struct BenchReport {
  pub total: usize,
  pub failed: usize,
  pub failures: Vec<(BenchDescription, Box<JsError>)>,
  pub measurements: Vec<(BenchDescription, BenchStats)>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Eq, Hash)]
pub struct BenchDescription {
  pub id: usize,
  pub name: String,
  pub origin: String,
  pub baseline: bool,
  pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchStats {
  pub n: u64,
  pub min: f64,
  pub max: f64,
  pub avg: f64,
  pub p75: f64,
  pub p99: f64,
  pub p995: f64,
  pub p999: f64,
}

impl BenchReport {
  pub fn new() -> Self {
    Self {
      total: 0,
      failed: 0,
      failures: Vec::new(),
      measurements: Vec::new(),
    }
  }
}

fn create_reporter(
  show_output: bool,
  json: bool,
) -> Box<dyn BenchReporter + Send> {
  if json {
    return Box::new(JsonReporter::new());
  }
  Box::new(ConsoleReporter::new(show_output))
}

pub trait BenchReporter {
  fn report_group_summary(&mut self);
  fn report_plan(&mut self, plan: &BenchPlan);
  fn report_end(&mut self, report: &BenchReport);
  fn report_register(&mut self, desc: &BenchDescription);
  fn report_wait(&mut self, desc: &BenchDescription);
  fn report_output(&mut self, output: &str);
  fn report_result(&mut self, desc: &BenchDescription, result: &BenchResult);
}

#[derive(Debug, Serialize)]
struct JsonReporterResult {
  runtime: String,
  cpu: String,
  origin: String,
  group: Option<String>,
  name: String,
  baseline: bool,
  result: BenchResult,
}

impl JsonReporterResult {
  fn new(
    origin: String,
    group: Option<String>,
    name: String,
    baseline: bool,
    result: BenchResult,
  ) -> Self {
    Self {
      runtime: format!("{} {}", get_user_agent(), env!("TARGET")),
      cpu: mitata::cpu::name(),
      origin,
      group,
      name,
      baseline,
      result,
    }
  }
}

#[derive(Debug, Serialize)]
struct JsonReporter(Vec<JsonReporterResult>);
impl JsonReporter {
  fn new() -> Self {
    Self(vec![])
  }
}

impl BenchReporter for JsonReporter {
  fn report_group_summary(&mut self) {}
  #[cold]
  fn report_plan(&mut self, _plan: &BenchPlan) {}

  fn report_end(&mut self, _report: &BenchReport) {
    match write_json_to_stdout(self) {
      Ok(_) => (),
      Err(e) => println!("{e}"),
    }
  }

  fn report_register(&mut self, _desc: &BenchDescription) {}

  fn report_wait(&mut self, _desc: &BenchDescription) {}

  fn report_output(&mut self, _output: &str) {}

  fn report_result(&mut self, desc: &BenchDescription, result: &BenchResult) {
    self.0.push(JsonReporterResult::new(
      desc.origin.clone(),
      desc.group.clone(),
      desc.name.clone(),
      desc.baseline,
      result.clone(),
    ));
  }
}

struct ConsoleReporter {
  name: String,
  show_output: bool,
  has_ungrouped: bool,
  group: Option<String>,
  baseline: bool,
  group_measurements: Vec<(BenchDescription, BenchStats)>,
  options: Option<mitata::reporter::Options>,
}

impl ConsoleReporter {
  fn new(show_output: bool) -> Self {
    Self {
      show_output,
      group: None,
      options: None,
      baseline: false,
      name: String::new(),
      has_ungrouped: false,
      group_measurements: Vec::new(),
    }
  }
}

impl BenchReporter for ConsoleReporter {
  #[cold]
  fn report_plan(&mut self, plan: &BenchPlan) {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    static FIRST_PLAN: AtomicBool = AtomicBool::new(true);

    self.report_group_summary();

    self.group = None;
    self.baseline = false;
    self.name = String::new();
    self.group_measurements.clear();
    self.options = Some(mitata::reporter::Options::new(
      &plan.names.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
    ));

    let options = self.options.as_mut().unwrap();

    options.percentiles = true;
    options.colors = colors::use_color();

    if FIRST_PLAN
      .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
      .is_ok()
    {
      println!("{}", colors::gray(format!("cpu: {}", mitata::cpu::name())));
      println!(
        "{}\n",
        colors::gray(format!(
          "runtime: deno {} ({})",
          crate::version::deno(),
          env!("TARGET")
        ))
      );
    } else {
      println!();
    }

    println!(
      "{}\n{}\n{}",
      colors::gray(&plan.origin),
      mitata::reporter::header(options),
      mitata::reporter::br(options)
    );
  }

  fn report_register(&mut self, _desc: &BenchDescription) {}

  fn report_wait(&mut self, desc: &BenchDescription) {
    self.name = desc.name.clone();

    match &desc.group {
      None => {
        self.has_ungrouped = true;
      }

      Some(group) => {
        if self.group.is_none()
          && self.has_ungrouped
          && self.group_measurements.is_empty()
        {
          println!();
        }

        if self.group.is_none() || group != self.group.as_ref().unwrap() {
          self.report_group_summary();
        }

        if (self.group.is_none() && self.has_ungrouped)
          || (self.group.is_some() && self.group_measurements.is_empty())
        {
          println!();
        }

        self.group = Some(group.clone());
      }
    }
  }

  fn report_output(&mut self, output: &str) {
    if self.show_output {
      print!("{} {}", colors::gray(format!("{}:", self.name)), output)
    }
  }

  fn report_result(&mut self, desc: &BenchDescription, result: &BenchResult) {
    let options = self.options.as_ref().unwrap();

    match result {
      BenchResult::Ok(stats) => {
        let mut desc = desc.clone();

        if desc.baseline && !self.baseline {
          self.baseline = true;
        } else {
          desc.baseline = false;
        }

        println!(
          "{}",
          mitata::reporter::benchmark(
            &desc.name,
            &mitata::reporter::BenchmarkStats {
              avg: stats.avg,
              min: stats.min,
              max: stats.max,
              p75: stats.p75,
              p99: stats.p99,
              p995: stats.p995,
            },
            options
          )
        );

        self.group_measurements.push((desc, stats.clone()));
      }

      BenchResult::Failed(js_error) => {
        println!(
          "{}",
          mitata::reporter::benchmark_error(
            &desc.name,
            &mitata::reporter::Error {
              stack: None,
              message: format_test_error(js_error),
            },
            options
          )
        )
      }
    };
  }

  fn report_group_summary(&mut self) {
    let options = match self.options.as_ref() {
      None => return,
      Some(options) => options,
    };

    if 2 <= self.group_measurements.len()
      && (self.group.is_some() || (self.group.is_none() && self.baseline))
    {
      println!(
        "\n{}",
        mitata::reporter::summary(
          &self
            .group_measurements
            .iter()
            .map(|(d, s)| mitata::reporter::GroupBenchmark {
              name: d.name.clone(),
              baseline: d.baseline,
              group: d.group.as_deref().unwrap_or("").to_owned(),

              stats: mitata::reporter::BenchmarkStats {
                avg: s.avg,
                min: s.min,
                max: s.max,
                p75: s.p75,
                p99: s.p99,
                p995: s.p995,
              },
            })
            .collect::<Vec<mitata::reporter::GroupBenchmark>>(),
          options
        )
      );
    }

    self.baseline = false;
    self.group_measurements.clear();
  }

  fn report_end(&mut self, _: &BenchReport) {
    self.report_group_summary();
  }
}

/// Type check a collection of module and document specifiers.
async fn check_specifiers(
  ps: &ProcState,
  permissions: Permissions,
  specifiers: Vec<ModuleSpecifier>,
) -> Result<(), AnyError> {
  let lib = ps.options.ts_type_lib_window();
  ps.prepare_module_load(
    specifiers,
    false,
    lib,
    PermissionsContainer::allow_all(),
    PermissionsContainer::new(permissions),
  )
  .await?;

  Ok(())
}

/// Run a single specifier as an executable bench module.
async fn bench_specifier(
  ps: ProcState,
  permissions: Permissions,
  specifier: ModuleSpecifier,
  channel: UnboundedSender<BenchEvent>,
  options: BenchSpecifierOptions,
) -> Result<(), AnyError> {
  let filter = options.filter;
  let mut worker = create_main_worker_for_test_or_bench(
    &ps,
    specifier,
    PermissionsContainer::new(permissions),
    vec![ops::bench::init(channel, filter)],
    Default::default(),
  )
  .await?;

  worker.run_bench_specifier().await
}

/// Test a collection of specifiers with test modes concurrently.
async fn bench_specifiers(
  ps: &ProcState,
  permissions: &Permissions,
  specifiers: Vec<ModuleSpecifier>,
  options: BenchSpecifierOptions,
) -> Result<(), AnyError> {
  let log_level = ps.options.log_level();

  let (sender, mut receiver) = unbounded_channel::<BenchEvent>();

  let option_for_handles = options.clone();

  let join_handles = specifiers.into_iter().map(move |specifier| {
    let ps = ps.clone();
    let permissions = permissions.clone();
    let specifier = specifier;
    let sender = sender.clone();
    let options = option_for_handles.clone();

    tokio::task::spawn_blocking(move || {
      let future = bench_specifier(ps, permissions, specifier, sender, options);

      run_local(future)
    })
  });

  let join_stream = stream::iter(join_handles)
    .buffer_unordered(1)
    .collect::<Vec<Result<Result<(), AnyError>, tokio::task::JoinError>>>();

  let handler = {
    tokio::task::spawn(async move {
      let mut used_only = false;
      let mut report = BenchReport::new();
      let mut reporter =
        create_reporter(log_level != Some(Level::Error), options.json);
      let mut benches = IndexMap::new();

      while let Some(event) = receiver.recv().await {
        match event {
          BenchEvent::Plan(plan) => {
            report.total += plan.total;
            if plan.used_only {
              used_only = true;
            }

            reporter.report_plan(&plan);
          }

          BenchEvent::Register(desc) => {
            reporter.report_register(&desc);
            benches.insert(desc.id, desc);
          }

          BenchEvent::Wait(id) => {
            reporter.report_wait(benches.get(&id).unwrap());
          }

          BenchEvent::Output(output) => {
            reporter.report_output(&output);
          }

          BenchEvent::Result(id, result) => {
            let desc = benches.get(&id).unwrap();
            reporter.report_result(desc, &result);
            match result {
              BenchResult::Ok(stats) => {
                report.measurements.push((desc.clone(), stats));
              }

              BenchResult::Failed(failure) => {
                report.failed += 1;
                report.failures.push((desc.clone(), failure));
              }
            };
          }
        }
      }

      reporter.report_end(&report);

      if used_only {
        return Err(generic_error(
          "Bench failed because the \"only\" option was used",
        ));
      }

      if report.failed > 0 {
        return Err(generic_error("Bench failed"));
      }

      Ok(())
    })
  };

  let (join_results, result) = future::join(join_stream, handler).await;

  // propagate any errors
  for join_result in join_results {
    join_result??;
  }

  result??;

  Ok(())
}

/// Checks if the path has a basename and extension Deno supports for benches.
fn is_supported_bench_path(path: &Path) -> bool {
  if let Some(name) = path.file_stem() {
    let basename = name.to_string_lossy();
    (basename.ends_with("_bench")
      || basename.ends_with(".bench")
      || basename == "bench")
      && is_supported_ext(path)
  } else {
    false
  }
}

pub async fn run_benchmarks(
  cli_options: CliOptions,
  bench_options: BenchOptions,
) -> Result<(), AnyError> {
  let ps = ProcState::from_options(Arc::new(cli_options)).await?;
  // Various bench files should not share the same permissions in terms of
  // `PermissionsContainer` - otherwise granting/revoking permissions in one
  // file would have impact on other files, which is undesirable.
  let permissions =
    Permissions::from_options(&ps.options.permissions_options())?;

  let specifiers =
    collect_specifiers(&bench_options.files, is_supported_bench_path)?;

  if specifiers.is_empty() {
    return Err(generic_error("No bench modules found"));
  }

  check_specifiers(&ps, permissions.clone(), specifiers.clone()).await?;

  bench_specifiers(
    &ps,
    &permissions,
    specifiers,
    BenchSpecifierOptions {
      filter: TestFilter::from_flag(&bench_options.filter),
      json: bench_options.json,
    },
  )
  .await?;

  Ok(())
}

// TODO(bartlomieju): heavy duplication of code with `cli/tools/test.rs`
pub async fn run_benchmarks_with_watch(
  cli_options: CliOptions,
  bench_options: BenchOptions,
) -> Result<(), AnyError> {
  let ps = ProcState::from_options(Arc::new(cli_options)).await?;
  // Various bench files should not share the same permissions in terms of
  // `PermissionsContainer` - otherwise granting/revoking permissions in one
  // file would have impact on other files, which is undesirable.
  let permissions =
    Permissions::from_options(&ps.options.permissions_options())?;
  let no_check = ps.options.type_check_mode() == TypeCheckMode::None;

  let ps = RefCell::new(ps);

  let resolver = |changed: Option<Vec<PathBuf>>| {
    let paths_to_watch = bench_options.files.include.clone();
    let paths_to_watch_clone = paths_to_watch.clone();
    let files_changed = changed.is_some();
    let bench_options = &bench_options;
    let ps = ps.borrow().clone();

    async move {
      let bench_modules =
        collect_specifiers(&bench_options.files, is_supported_bench_path)?;

      let mut paths_to_watch = paths_to_watch_clone;
      let mut modules_to_reload = if files_changed {
        Vec::new()
      } else {
        bench_modules.clone()
      };
      let graph = ps.create_graph(bench_modules.clone()).await?;
      graph_valid_with_cli_options(&graph, &bench_modules, &ps.options)?;

      // TODO(@kitsonk) - This should be totally derivable from the graph.
      for specifier in bench_modules {
        fn get_dependencies<'a>(
          graph: &'a deno_graph::ModuleGraph,
          maybe_module: Option<&'a deno_graph::Module>,
          // This needs to be accessible to skip getting dependencies if they're already there,
          // otherwise this will cause a stack overflow with circular dependencies
          output: &mut HashSet<&'a ModuleSpecifier>,
          no_check: bool,
        ) {
          if let Some(module) = maybe_module {
            for dep in module.dependencies.values() {
              if let Some(specifier) = &dep.get_code() {
                if !output.contains(specifier) {
                  output.insert(specifier);
                  get_dependencies(
                    graph,
                    graph.get(specifier),
                    output,
                    no_check,
                  );
                }
              }
              if !no_check {
                if let Some(specifier) = &dep.get_type() {
                  if !output.contains(specifier) {
                    output.insert(specifier);
                    get_dependencies(
                      graph,
                      graph.get(specifier),
                      output,
                      no_check,
                    );
                  }
                }
              }
            }
          }
        }
        // This bench module and all it's dependencies
        let mut modules = HashSet::new();
        modules.insert(&specifier);
        get_dependencies(&graph, graph.get(&specifier), &mut modules, no_check);

        paths_to_watch.extend(
          modules
            .iter()
            .filter_map(|specifier| specifier.to_file_path().ok()),
        );

        if let Some(changed) = &changed {
          for path in changed.iter().filter_map(|path| {
            deno_core::resolve_url_or_path(&path.to_string_lossy()).ok()
          }) {
            if modules.contains(&path) {
              modules_to_reload.push(specifier);
              break;
            }
          }
        }
      }

      Ok((paths_to_watch, modules_to_reload))
    }
    .map(move |result| {
      if files_changed
        && matches!(result, Ok((_, ref modules)) if modules.is_empty())
      {
        ResolutionResult::Ignore
      } else {
        match result {
          Ok((paths_to_watch, modules_to_reload)) => {
            ResolutionResult::Restart {
              paths_to_watch,
              result: Ok(modules_to_reload),
            }
          }
          Err(e) => ResolutionResult::Restart {
            paths_to_watch,
            result: Err(e),
          },
        }
      }
    })
  };

  let operation = |modules_to_reload: Vec<ModuleSpecifier>| {
    let permissions = &permissions;
    let bench_options = &bench_options;
    ps.borrow_mut().reset_for_file_watcher();
    let ps = ps.borrow().clone();

    async move {
      let specifiers =
        collect_specifiers(&bench_options.files, is_supported_bench_path)?
          .into_iter()
          .filter(|specifier| modules_to_reload.contains(specifier))
          .collect::<Vec<ModuleSpecifier>>();

      check_specifiers(&ps, permissions.clone(), specifiers.clone()).await?;

      bench_specifiers(
        &ps,
        permissions,
        specifiers,
        BenchSpecifierOptions {
          filter: TestFilter::from_flag(&bench_options.filter),
          json: bench_options.json,
        },
      )
      .await?;

      Ok(())
    }
  };

  let clear_screen = !ps.borrow().options.no_clear_screen();
  file_watcher::watch_func(
    resolver,
    operation,
    file_watcher::PrintConfig {
      job_name: "Bench".to_string(),
      clear_screen,
    },
  )
  .await?;

  Ok(())
}
