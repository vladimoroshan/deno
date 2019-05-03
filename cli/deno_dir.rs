// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
use crate::compiler::ModuleMetaData;
use crate::errors;
use crate::errors::DenoError;
use crate::errors::DenoResult;
use crate::errors::ErrorKind;
use crate::fs as deno_fs;
use crate::http_util;
use crate::js_errors::SourceMapGetter;
use crate::msg;
use crate::tokio_util;
use crate::version;
use dirs;
use futures::future::{loop_fn, Either, Loop};
use futures::Future;
use http;
use ring;
use serde_json;
use std;
use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::str;
use url;
use url::Url;

/// Gets corresponding MediaType given extension
fn extmap(ext: &str) -> msg::MediaType {
  match ext {
    "ts" => msg::MediaType::TypeScript,
    "js" => msg::MediaType::JavaScript,
    "json" => msg::MediaType::Json,
    _ => msg::MediaType::Unknown,
  }
}

#[derive(Clone)]
pub struct DenoDir {
  // Example: /Users/rld/.deno/
  pub root: PathBuf,
  // In the Go code this was called SrcDir.
  // This is where we cache http resources. Example:
  // /Users/rld/.deno/deps/github.com/ry/blah.js
  pub gen: PathBuf,
  // In the Go code this was called CacheDir.
  // This is where we cache compilation outputs. Example:
  // /Users/rld/.deno/gen/f39a473452321cacd7c346a870efb0e3e1264b43.js
  pub deps: PathBuf,
  // This splits to http and https deps
  pub deps_http: PathBuf,
  pub deps_https: PathBuf,
  /// The active configuration file contents (or empty array) which applies to
  /// source code cached by `DenoDir`.
  pub config: Vec<u8>,
}

impl DenoDir {
  // Must be called before using any function from this module.
  // https://github.com/denoland/deno/blob/golang/deno_dir.go#L99-L111
  pub fn new(
    custom_root: Option<PathBuf>,
    state_config: &Option<Vec<u8>>,
  ) -> std::io::Result<Self> {
    // Only setup once.
    let home_dir = dirs::home_dir().expect("Could not get home directory.");
    let fallback = home_dir.join(".deno");
    // We use the OS cache dir because all files deno writes are cache files
    // Once that changes we need to start using different roots if DENO_DIR
    // is not set, and keep a single one if it is.
    let default = dirs::cache_dir()
      .map(|d| d.join("deno"))
      .unwrap_or(fallback);

    let root: PathBuf = custom_root.unwrap_or(default);
    let gen = root.as_path().join("gen");
    let deps = root.as_path().join("deps");
    let deps_http = deps.join("http");
    let deps_https = deps.join("https");

    // Internally within DenoDir, we use the config as part of the hash to
    // determine if a file has been transpiled with the same configuration, but
    // we have borrowed the `State` configuration, which we want to either clone
    // or create an empty `Vec` which we will use in our hash function.
    let config = match state_config {
      Some(config) => config.clone(),
      _ => b"".to_vec(),
    };

    let deno_dir = Self {
      root,
      gen,
      deps,
      deps_http,
      deps_https,
      config,
    };

    // TODO Lazily create these directories.
    deno_fs::mkdir(deno_dir.gen.as_ref(), 0o755, true)?;
    deno_fs::mkdir(deno_dir.deps.as_ref(), 0o755, true)?;
    deno_fs::mkdir(deno_dir.deps_http.as_ref(), 0o755, true)?;
    deno_fs::mkdir(deno_dir.deps_https.as_ref(), 0o755, true)?;

    debug!("root {}", deno_dir.root.display());
    debug!("gen {}", deno_dir.gen.display());
    debug!("deps {}", deno_dir.deps.display());
    debug!("deps_http {}", deno_dir.deps_http.display());
    debug!("deps_https {}", deno_dir.deps_https.display());

    Ok(deno_dir)
  }

  // https://github.com/denoland/deno/blob/golang/deno_dir.go#L32-L35
  pub fn cache_path(
    self: &Self,
    filename: &str,
    source_code: &[u8],
  ) -> (PathBuf, PathBuf) {
    let cache_key =
      source_code_hash(filename, source_code, version::DENO, &self.config);
    (
      self.gen.join(cache_key.to_string() + ".js"),
      self.gen.join(cache_key.to_string() + ".js.map"),
    )
  }

  pub fn code_cache(
    self: &Self,
    module_meta_data: &ModuleMetaData,
  ) -> std::io::Result<()> {
    let (cache_path, source_map_path) = self
      .cache_path(&module_meta_data.filename, &module_meta_data.source_code);
    // TODO(ry) This is a race condition w.r.t to exists() -- probably should
    // create the file in exclusive mode. A worry is what might happen is there
    // are two processes and one reads the cache file while the other is in the
    // midst of writing it.
    if cache_path.exists() && source_map_path.exists() {
      Ok(())
    } else {
      match &module_meta_data.maybe_output_code {
        Some(output_code) => fs::write(cache_path, output_code),
        _ => Ok(()),
      }?;
      match &module_meta_data.maybe_source_map {
        Some(source_map) => fs::write(source_map_path, source_map),
        _ => Ok(()),
      }?;
      Ok(())
    }
  }

  pub fn fetch_module_meta_data_async(
    self: &Self,
    specifier: &str,
    referrer: &str,
    use_cache: bool,
    no_fetch: bool,
  ) -> impl Future<Item = ModuleMetaData, Error = errors::DenoError> {
    debug!(
      "fetch_module_meta_data. specifier {} referrer {}",
      specifier, referrer
    );

    let specifier = specifier.to_string();
    let referrer = referrer.to_string();

    let result = self.resolve_module(&specifier, &referrer);
    if let Err(err) = result {
      return Either::A(futures::future::err(DenoError::from(err)));
    }
    let (module_name, filename) = result.unwrap();

    let gen = self.gen.clone();

    // If we don't clone the config, we then end up creating an implied lifetime
    // which gets returned in the future, so we clone here so as to not leak the
    // move below when the future is resolving.
    let config = self.config.clone();

    Either::B(
      get_source_code_async(
        self,
        module_name.as_str(),
        filename.as_str(),
        use_cache,
        no_fetch,
      ).then(move |result| {
        let mut out = match result {
          Ok(out) => out,
          Err(err) => {
            if err.kind() == ErrorKind::NotFound {
              // For NotFound, change the message to something better.
              return Err(errors::new(
                ErrorKind::NotFound,
                format!(
                  "Cannot resolve module \"{}\" from \"{}\"",
                  specifier, referrer
                ),
              ));
            } else {
              return Err(err);
            }
          }
        };

        if out.source_code.starts_with(b"#!") {
          out.source_code = filter_shebang(out.source_code);
        }

        // If TypeScript we have to also load corresponding compile js and
        // source maps (called output_code and output_source_map)
        if out.media_type != msg::MediaType::TypeScript || !use_cache {
          return Ok(out);
        }

        let cache_key = source_code_hash(
          &out.filename,
          &out.source_code,
          version::DENO,
          &config,
        );
        let (output_code_filename, output_source_map_filename) = (
          gen.join(cache_key.to_string() + ".js"),
          gen.join(cache_key.to_string() + ".js.map"),
        );

        let result =
          load_cache2(&output_code_filename, &output_source_map_filename);
        match result {
          Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
              // If there's no compiled JS or source map, that's ok, just
              // return what we have.
              Ok(out)
            } else {
              Err(err.into())
            }
          }
          Ok((output_code, source_map)) => {
            out.maybe_output_code = Some(output_code);
            out.maybe_source_map = Some(source_map);
            out.maybe_output_code_filename =
              Some(output_code_filename.to_str().unwrap().to_string());
            out.maybe_source_map_filename =
              Some(output_source_map_filename.to_str().unwrap().to_string());
            Ok(out)
          }
        }
      }),
    )
  }

  /// Synchronous version of fetch_module_meta_data_async
  /// This function is deprecated.
  pub fn fetch_module_meta_data(
    self: &Self,
    specifier: &str,
    referrer: &str,
    use_cache: bool,
    no_fetch: bool,
  ) -> Result<ModuleMetaData, errors::DenoError> {
    tokio_util::block_on(
      self
        .fetch_module_meta_data_async(specifier, referrer, use_cache, no_fetch),
    )
  }

  // Prototype: https://github.com/denoland/deno/blob/golang/os.go#L56-L68
  fn src_file_to_url(self: &Self, filename: &str) -> String {
    let filename_path = Path::new(filename);
    if filename_path.starts_with(&self.deps) {
      let (rest, prefix) = if filename_path.starts_with(&self.deps_https) {
        let rest = filename_path.strip_prefix(&self.deps_https).unwrap();
        let prefix = "https://".to_string();
        (rest, prefix)
      } else if filename_path.starts_with(&self.deps_http) {
        let rest = filename_path.strip_prefix(&self.deps_http).unwrap();
        let prefix = "http://".to_string();
        (rest, prefix)
      } else {
        // TODO(kevinkassimo): change this to support other protocols than http
        unimplemented!()
      };
      // Windows doesn't support ":" in filenames, so we represent port using a
      // special string.
      // TODO(ry) This current implementation will break on a URL that has
      // the default port but contains "_PORT" in the path.
      let rest = rest.to_str().unwrap().replacen("_PORT", ":", 1);
      prefix + &rest
    } else {
      String::from(filename)
    }
  }

  /// Returns (module name, local filename)
  pub fn resolve_module_url(
    self: &Self,
    specifier: &str,
    referrer: &str,
  ) -> Result<Url, url::ParseError> {
    let specifier = self.src_file_to_url(specifier);
    let mut referrer = self.src_file_to_url(referrer);

    debug!(
      "resolve_module specifier {} referrer {}",
      specifier, referrer
    );

    if referrer.starts_with('.') {
      let cwd = std::env::current_dir().unwrap();
      let referrer_path = cwd.join(referrer);
      referrer = referrer_path.to_str().unwrap().to_string() + "/";
    }

    let j = if is_remote(&specifier)
      || (Path::new(&specifier).is_absolute() && !is_remote(&referrer))
    {
      parse_local_or_remote(&specifier)?
    } else if referrer.ends_with('/') {
      let r = Url::from_directory_path(&referrer);
      // TODO(ry) Properly handle error.
      if r.is_err() {
        error!("Url::from_directory_path error {}", referrer);
      }
      let base = r.unwrap();
      base.join(specifier.as_ref())?
    } else {
      let base = parse_local_or_remote(&referrer)?;
      base.join(specifier.as_ref())?
    };
    Ok(j)
  }

  /// Returns (module name, local filename)
  pub fn resolve_module(
    self: &Self,
    specifier: &str,
    referrer: &str,
  ) -> Result<(String, String), url::ParseError> {
    let j = self.resolve_module_url(specifier, referrer)?;

    let module_name = j.to_string();
    let filename;
    match j.scheme() {
      "file" => {
        filename = deno_fs::normalize_path(j.to_file_path().unwrap().as_ref());
      }
      "https" => {
        filename = deno_fs::normalize_path(
          get_cache_filename(self.deps_https.as_path(), &j).as_ref(),
        )
      }
      "http" => {
        filename = deno_fs::normalize_path(
          get_cache_filename(self.deps_http.as_path(), &j).as_ref(),
        )
      }
      // TODO(kevinkassimo): change this to support other protocols than http.
      _ => unimplemented!(),
    }

    debug!("module_name: {}, filename: {}", module_name, filename);
    Ok((module_name, filename))
  }
}

impl SourceMapGetter for DenoDir {
  fn get_source_map(&self, script_name: &str) -> Option<Vec<u8>> {
    match self.fetch_module_meta_data(script_name, ".", true, true) {
      Err(_e) => None,
      Ok(out) => match out.maybe_source_map {
        None => None,
        Some(source_map) => Some(source_map),
      },
    }
  }
}

/// This fetches source code, locally or remotely.
/// module_name is the URL specifying the module.
/// filename is the local path to the module (if remote, it is in the cache
/// folder, and potentially does not exist yet)
///
/// It *does not* fill the compiled JS nor source map portions of
/// ModuleMetaData. This is the only difference between this function and
/// fetch_module_meta_data_async(). TODO(ry) change return type to reflect this
/// fact.
///
/// If this is a remote module, and it has not yet been cached, the resulting
/// download will be written to "filename". This happens no matter the value of
/// use_cache.
fn get_source_code_async(
  deno_dir: &DenoDir,
  module_name: &str,
  filename: &str,
  use_cache: bool,
  no_fetch: bool,
) -> impl Future<Item = ModuleMetaData, Error = DenoError> {
  let filename = filename.to_string();
  let module_name = module_name.to_string();
  let is_module_remote = is_remote(&module_name);
  // We try fetch local. Three cases:
  // 1. Remote downloads are not allowed, we're only allowed to use cache.
  // 2. This is a remote module and we're allowed to use cached downloads.
  // 3. This is a local module.
  if !is_module_remote || use_cache || no_fetch {
    debug!(
      "fetch local or reload {} is_module_remote {}",
      module_name, is_module_remote
    );
    // Note that local fetch is done synchronously.
    match fetch_local_source(deno_dir, &module_name, &filename, None) {
      Ok(Some(output)) => {
        debug!("found local source ");
        return Either::A(futures::future::ok(output));
      }
      Ok(None) => {
        debug!("fetch_local_source returned None");
      }
      Err(err) => {
        return Either::A(futures::future::err(err));
      }
    }
  }

  // If not remote file stop here!
  if !is_module_remote {
    debug!("not remote file stop here");
    return Either::A(futures::future::err(DenoError::from(
      std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("cannot find local file '{}'", &filename),
      ),
    )));
  }

  // If remote downloads are not allowed stop here!
  if no_fetch {
    debug!("remote file with no_fetch stop here");
    return Either::A(futures::future::err(DenoError::from(
      std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("cannot find remote file '{}' in cache", &filename),
      ),
    )));
  }

  debug!("is remote but didn't find module");

  // not cached/local, try remote.
  Either::B(
    fetch_remote_source_async(deno_dir, &module_name, &filename).and_then(
      move |maybe_remote_source| match maybe_remote_source {
        Some(output) => Ok(output),
        None => Err(DenoError::from(std::io::Error::new(
          std::io::ErrorKind::NotFound,
          format!("cannot find remote file '{}'", &filename),
        ))),
      },
    ),
  )
}

#[cfg(test)]
/// Synchronous version of get_source_code_async
/// This function is deprecated.
fn get_source_code(
  deno_dir: &DenoDir,
  module_name: &str,
  filename: &str,
  use_cache: bool,
  no_fetch: bool,
) -> DenoResult<ModuleMetaData> {
  tokio_util::block_on(get_source_code_async(
    deno_dir,
    module_name,
    filename,
    use_cache,
    no_fetch,
  ))
}

fn get_cache_filename(basedir: &Path, url: &Url) -> PathBuf {
  let host = url.host_str().unwrap();
  let host_port = match url.port() {
    // Windows doesn't support ":" in filenames, so we represent port using a
    // special string.
    Some(port) => format!("{}_PORT{}", host, port),
    None => host.to_string(),
  };

  let mut out = basedir.to_path_buf();
  out.push(host_port);
  for path_seg in url.path_segments().unwrap() {
    out.push(path_seg);
  }
  out
}

fn load_cache2(
  js_filename: &PathBuf,
  map_filename: &PathBuf,
) -> Result<(Vec<u8>, Vec<u8>), std::io::Error> {
  debug!(
    "load_cache code: {} map: {}",
    js_filename.display(),
    map_filename.display()
  );
  let read_output_code = fs::read(&js_filename)?;
  let read_source_map = fs::read(&map_filename)?;
  Ok((read_output_code, read_source_map))
}

/// Generate an SHA1 hash for source code, to be used to determine if a cached
/// version of the code is valid or invalid.
fn source_code_hash(
  filename: &str,
  source_code: &[u8],
  version: &str,
  config: &[u8],
) -> String {
  let mut ctx = ring::digest::Context::new(&ring::digest::SHA1);
  ctx.update(version.as_bytes());
  ctx.update(filename.as_bytes());
  ctx.update(source_code);
  ctx.update(config);
  let digest = ctx.finish();
  let mut out = String::new();
  // TODO There must be a better way to do this...
  for byte in digest.as_ref() {
    write!(&mut out, "{:02x}", byte).unwrap();
  }
  out
}

fn is_remote(module_name: &str) -> bool {
  module_name.starts_with("http://") || module_name.starts_with("https://")
}

fn parse_local_or_remote(p: &str) -> Result<url::Url, url::ParseError> {
  if is_remote(p) || p.starts_with("file:") {
    Url::parse(p)
  } else {
    Url::from_file_path(p).map_err(|_err| url::ParseError::IdnaError)
  }
}

fn map_file_extension(path: &Path) -> msg::MediaType {
  match path.extension() {
    None => msg::MediaType::Unknown,
    Some(os_str) => match os_str.to_str() {
      Some("ts") => msg::MediaType::TypeScript,
      Some("js") => msg::MediaType::JavaScript,
      Some("json") => msg::MediaType::Json,
      _ => msg::MediaType::Unknown,
    },
  }
}

// convert a ContentType string into a enumerated MediaType
fn map_content_type(path: &Path, content_type: Option<&str>) -> msg::MediaType {
  match content_type {
    Some(content_type) => {
      // sometimes there is additional data after the media type in
      // Content-Type so we have to do a bit of manipulation so we are only
      // dealing with the actual media type
      let ct_vector: Vec<&str> = content_type.split(';').collect();
      let ct: &str = ct_vector.first().unwrap();
      match ct.to_lowercase().as_ref() {
        "application/typescript"
        | "text/typescript"
        | "video/vnd.dlna.mpeg-tts"
        | "video/mp2t"
        | "application/x-typescript" => msg::MediaType::TypeScript,
        "application/javascript"
        | "text/javascript"
        | "application/ecmascript"
        | "text/ecmascript"
        | "application/x-javascript" => msg::MediaType::JavaScript,
        "application/json" | "text/json" => msg::MediaType::Json,
        "text/plain" => map_file_extension(path),
        _ => {
          debug!("unknown content type: {}", content_type);
          msg::MediaType::Unknown
        }
      }
    }
    None => map_file_extension(path),
  }
}

fn filter_shebang(bytes: Vec<u8>) -> Vec<u8> {
  let string = str::from_utf8(&bytes).unwrap();
  if let Some(i) = string.find('\n') {
    let (_, rest) = string.split_at(i);
    rest.as_bytes().to_owned()
  } else {
    Vec::new()
  }
}

/// Asynchronously fetch remote source file specified by the URL `module_name`
/// and write it to disk at `filename`.
fn fetch_remote_source_async(
  deno_dir: &DenoDir,
  module_name: &str,
  filename: &str,
) -> impl Future<Item = Option<ModuleMetaData>, Error = DenoError> {
  use crate::http_util::FetchOnceResult;
  {
    eprintln!("Downloading {}", module_name);
  }

  let filename = filename.to_owned();
  let module_name = module_name.to_owned();

  // We write a special ".headers.json" file into the `.deno/deps` directory along side the
  // cached file, containing just the media type and possible redirect target (both are http headers).
  // If redirect target is present, the file itself if not cached.
  // In future resolutions, we would instead follow this redirect target ("redirect_to").
  loop_fn(
    (
      deno_dir.clone(),
      None,
      None,
      module_name.clone(),
      filename.clone(),
    ),
    |(
      dir,
      mut maybe_initial_module_name,
      mut maybe_initial_filename,
      module_name,
      filename,
    )| {
      let url = module_name.parse::<http::uri::Uri>().unwrap();
      // Single pass fetch, either yields code or yields redirect.
      http_util::fetch_string_once(url).and_then(move |fetch_once_result| {
        match fetch_once_result {
          FetchOnceResult::Redirect(url) => {
            // If redirects, update module_name and filename for next looped call.
            let resolve_result = dir
              .resolve_module(&(url.to_string()), ".")
              .map_err(DenoError::from);
            match resolve_result {
              Ok((new_module_name, new_filename)) => {
                if maybe_initial_module_name.is_none() {
                  maybe_initial_module_name = Some(module_name.clone());
                  maybe_initial_filename = Some(filename.clone());
                }
                // Not yet completed. Follow the redirect and loop.
                Ok(Loop::Continue((
                  dir,
                  maybe_initial_module_name,
                  maybe_initial_filename,
                  new_module_name,
                  new_filename,
                )))
              }
              Err(e) => Err(e),
            }
          }
          FetchOnceResult::Code(source, maybe_content_type) => {
            // We land on the code.
            let p = PathBuf::from(filename.clone());
            match p.parent() {
              Some(ref parent) => fs::create_dir_all(parent),
              None => Ok(()),
            }?;
            // Write file and create .headers.json for the file.
            deno_fs::write_file(&p, &source, 0o666)?;
            {
              save_source_code_headers(
                &filename,
                maybe_content_type.clone(),
                None,
              );
            }
            // Check if this file is downloaded due to some old redirect request.
            if maybe_initial_filename.is_some() {
              // If yes, record down the headers for redirect.
              // Also create its containing folder.
              let pp = PathBuf::from(filename.clone());
              match pp.parent() {
                Some(ref parent) => fs::create_dir_all(parent),
                None => Ok(()),
              }?;
              {
                save_source_code_headers(
                  &maybe_initial_filename.clone().unwrap(),
                  maybe_content_type.clone(),
                  Some(module_name.clone()),
                );
              }
            }
            Ok(Loop::Break(Some(ModuleMetaData {
              module_name: module_name.to_string(),
              module_redirect_source_name: maybe_initial_module_name,
              filename: filename.to_string(),
              media_type: map_content_type(
                &p,
                maybe_content_type.as_ref().map(String::as_str),
              ),
              source_code: source.as_bytes().to_owned(),
              maybe_output_code_filename: None,
              maybe_output_code: None,
              maybe_source_map_filename: None,
              maybe_source_map: None,
            })))
          }
        }
      })
    },
  )
}

/// Fetch remote source code.
#[cfg(test)]
fn fetch_remote_source(
  deno_dir: &DenoDir,
  module_name: &str,
  filename: &str,
) -> DenoResult<Option<ModuleMetaData>> {
  tokio_util::block_on(fetch_remote_source_async(
    deno_dir,
    module_name,
    filename,
  ))
}

/// Fetch local or cached source code.
/// This is a recursive operation if source file has redirection.
/// It will keep reading filename.headers.json for information about redirection.
/// module_initial_source_name would be None on first call,
/// and becomes the name of the very first module that initiates the call
/// in subsequent recursions.
/// AKA if redirection occurs, module_initial_source_name is the source path
/// that user provides, and the final module_name is the resolved path
/// after following all redirections.
fn fetch_local_source(
  deno_dir: &DenoDir,
  module_name: &str,
  filename: &str,
  module_initial_source_name: Option<String>,
) -> DenoResult<Option<ModuleMetaData>> {
  let p = Path::new(&filename);
  let source_code_headers = get_source_code_headers(&filename);
  // If source code headers says that it would redirect elsewhere,
  // (meaning that the source file might not exist; only .headers.json is present)
  // Abort reading attempts to the cached source file and and follow the redirect.
  if let Some(redirect_to) = source_code_headers.redirect_to {
    // E.g.
    // module_name https://import-meta.now.sh/redirect.js
    // filename /Users/kun/Library/Caches/deno/deps/https/import-meta.now.sh/redirect.js
    // redirect_to https://import-meta.now.sh/sub/final1.js
    // real_filename /Users/kun/Library/Caches/deno/deps/https/import-meta.now.sh/sub/final1.js
    // real_module_name = https://import-meta.now.sh/sub/final1.js
    let (real_module_name, real_filename) =
      deno_dir.resolve_module(&redirect_to, ".")?;
    let mut module_initial_source_name = module_initial_source_name;
    // If this is the first redirect attempt,
    // then module_initial_source_name should be None.
    // In that case, use current module name as module_initial_source_name.
    if module_initial_source_name.is_none() {
      module_initial_source_name = Some(module_name.to_owned());
    }
    // Recurse.
    return fetch_local_source(
      deno_dir,
      &real_module_name,
      &real_filename,
      module_initial_source_name,
    );
  }
  // No redirect needed or end of redirects.
  // We can try read the file
  let source_code = match fs::read(p) {
    Err(e) => {
      if e.kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
      } else {
        return Err(e.into());
      }
    }
    Ok(c) => c,
  };
  Ok(Some(ModuleMetaData {
    module_name: module_name.to_string(),
    module_redirect_source_name: module_initial_source_name,
    filename: filename.to_string(),
    media_type: map_content_type(
      &p,
      source_code_headers.mime_type.as_ref().map(String::as_str),
    ),
    source_code,
    maybe_output_code_filename: None,
    maybe_output_code: None,
    maybe_source_map_filename: None,
    maybe_source_map: None,
  }))
}

#[derive(Debug)]
/// Header metadata associated with a particular "symbolic" source code file.
/// (the associated source code file might not be cached, while remaining
/// a user accessible entity through imports (due to redirects)).
pub struct SourceCodeHeaders {
  /// MIME type of the source code.
  pub mime_type: Option<String>,
  /// Where should we actually look for source code.
  /// This should be an absolute path!
  pub redirect_to: Option<String>,
}

static MIME_TYPE: &'static str = "mime_type";
static REDIRECT_TO: &'static str = "redirect_to";

fn source_code_headers_filename(filename: &str) -> String {
  [&filename, ".headers.json"].concat()
}

/// Get header metadata associated with a single source code file.
/// NOTICE: chances are that the source code itself is not downloaded due to redirects.
/// In this case, the headers file provides info about where we should go and get
/// the source code that redirect eventually points to (which should be cached).
fn get_source_code_headers(filename: &str) -> SourceCodeHeaders {
  let headers_filename = source_code_headers_filename(filename);
  let hd = Path::new(&headers_filename);
  // .headers.json file might not exists.
  // This is okay for local source.
  let maybe_headers_string = fs::read_to_string(&hd).ok();
  if let Some(headers_string) = maybe_headers_string {
    // TODO(kevinkassimo): consider introduce serde::Deserialize to make things simpler.
    let maybe_headers: serde_json::Result<serde_json::Value> =
      serde_json::from_str(&headers_string);
    if let Ok(headers) = maybe_headers {
      return SourceCodeHeaders {
        mime_type: headers[MIME_TYPE].as_str().map(String::from),
        redirect_to: headers[REDIRECT_TO].as_str().map(String::from),
      };
    }
  }
  SourceCodeHeaders {
    mime_type: None,
    redirect_to: None,
  }
}

/// Save headers related to source filename to {filename}.headers.json file,
/// only when there is actually something necessary to save.
/// For example, if the extension ".js" already mean JS file and we have
/// content type of "text/javascript", then we would not save the mime type.
/// If nothing needs to be saved, the headers file is not created.
fn save_source_code_headers(
  filename: &str,
  mime_type: Option<String>,
  redirect_to: Option<String>,
) {
  let headers_filename = source_code_headers_filename(filename);
  // Remove possibly existing stale .headers.json file.
  // May not exist. DON'T unwrap.
  let _ = std::fs::remove_file(&headers_filename);
  let p = PathBuf::from(filename);
  // TODO(kevinkassimo): consider introduce serde::Deserialize to make things simpler.
  // This is super ugly at this moment...
  // Had trouble to make serde_derive work: I'm unable to build proc-macro2.
  let mut value_map = serde_json::map::Map::new();
  if mime_type.is_some() {
    let mime_type_string = mime_type.clone().unwrap();
    let resolved_mime_type =
      { map_content_type(Path::new(""), Some(mime_type_string.as_str())) };
    let ext = p
      .extension()
      .map(|x| x.to_str().unwrap_or(""))
      .unwrap_or("");
    let ext_based_mime_type = extmap(&ext);
    // Add mime to headers only when content type is different from extension.
    if ext_based_mime_type == msg::MediaType::Unknown
      || resolved_mime_type != ext_based_mime_type
    {
      value_map.insert(MIME_TYPE.to_string(), json!(mime_type_string));
    }
  }
  if redirect_to.is_some() {
    value_map.insert(REDIRECT_TO.to_string(), json!(redirect_to.unwrap()));
  }
  // Only save to file when there is actually data.
  if !value_map.is_empty() {
    let _ = serde_json::to_string(&value_map).map(|s| {
      // It is possible that we need to create file
      // with parent folders not yet created.
      // (Due to .headers.json feature for redirection)
      let hd = PathBuf::from(&headers_filename);
      let _ = match hd.parent() {
        Some(ref parent) => fs::create_dir_all(parent),
        None => Ok(()),
      };
      let _ = deno_fs::write_file(&(hd.as_path()), s, 0o666);
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  fn test_setup() -> (TempDir, DenoDir) {
    let temp_dir = TempDir::new().expect("tempdir fail");
    let config = Some(b"{}".to_vec());
    let deno_dir = DenoDir::new(Some(temp_dir.path().to_path_buf()), &config)
      .expect("setup fail");
    (temp_dir, deno_dir)
  }

  // The `add_root` macro prepends "C:" to a string if on windows; on posix
  // systems it returns the input string untouched. This is necessary because
  // `Url::from_file_path()` fails if the input path isn't an absolute path.
  macro_rules! add_root {
    ($path:expr) => {
      if cfg!(target_os = "windows") {
        concat!("C:", $path)
      } else {
        $path
      }
    };
  }

  macro_rules! file_url {
    ($path:expr) => {
      if cfg!(target_os = "windows") {
        concat!("file:///C:", $path)
      } else {
        concat!("file://", $path)
      }
    };
  }

  #[test]
  fn test_get_cache_filename() {
    let url = Url::parse("http://example.com:1234/path/to/file.ts").unwrap();
    let basedir = Path::new("/cache/dir/");
    let cache_file = get_cache_filename(&basedir, &url);
    assert_eq!(
      cache_file,
      Path::new("/cache/dir/example.com_PORT1234/path/to/file.ts")
    );
  }

  #[test]
  fn test_cache_path() {
    let (temp_dir, deno_dir) = test_setup();
    let filename = "hello.js";
    let source_code = b"1+2";
    let config = b"{}";
    let hash = source_code_hash(filename, source_code, version::DENO, config);
    assert_eq!(
      (
        temp_dir.path().join(format!("gen/{}.js", hash)),
        temp_dir.path().join(format!("gen/{}.js.map", hash))
      ),
      deno_dir.cache_path(filename, source_code)
    );
  }

  #[test]
  fn test_cache_path_config() {
    // We are changing the compiler config from the "mock" and so we expect the
    // resolved files coming back to not match the calculated hash.
    let (temp_dir, deno_dir) = test_setup();
    let filename = "hello.js";
    let source_code = b"1+2";
    let config = b"{\"compilerOptions\":{}}";
    let hash = source_code_hash(filename, source_code, version::DENO, config);
    assert_ne!(
      (
        temp_dir.path().join(format!("gen/{}.js", hash)),
        temp_dir.path().join(format!("gen/{}.js.map", hash))
      ),
      deno_dir.cache_path(filename, source_code)
    );
  }

  #[test]
  fn test_code_cache() {
    let (_temp_dir, deno_dir) = test_setup();

    let filename = "hello.js";
    let source_code = b"1+2";
    let output_code = b"1+2 // output code";
    let source_map = b"{}";
    let config = b"{}";
    let hash = source_code_hash(filename, source_code, version::DENO, config);
    let (cache_path, source_map_path) =
      deno_dir.cache_path(filename, source_code);
    assert!(cache_path.ends_with(format!("gen/{}.js", hash)));
    assert!(source_map_path.ends_with(format!("gen/{}.js.map", hash)));

    let out = ModuleMetaData {
      filename: filename.to_owned(),
      source_code: source_code[..].to_owned(),
      module_name: "hello.js".to_owned(),
      module_redirect_source_name: None,
      media_type: msg::MediaType::TypeScript,
      maybe_output_code: Some(output_code[..].to_owned()),
      maybe_output_code_filename: None,
      maybe_source_map: Some(source_map[..].to_owned()),
      maybe_source_map_filename: None,
    };

    let r = deno_dir.code_cache(&out);
    r.expect("code_cache error");
    assert!(cache_path.exists());
    assert_eq!(output_code[..].to_owned(), fs::read(&cache_path).unwrap());
  }

  #[test]
  fn test_source_code_hash() {
    assert_eq!(
      "830c8b63ba3194cf2108a3054c176b2bf53aee45",
      source_code_hash("hello.ts", b"1+2", "0.2.11", b"{}")
    );
    // Different source_code should result in different hash.
    assert_eq!(
      "fb06127e9b2e169bea9c697fa73386ae7c901e8b",
      source_code_hash("hello.ts", b"1", "0.2.11", b"{}")
    );
    // Different filename should result in different hash.
    assert_eq!(
      "3a17b6a493ff744b6a455071935f4bdcd2b72ec7",
      source_code_hash("hi.ts", b"1+2", "0.2.11", b"{}")
    );
    // Different version should result in different hash.
    assert_eq!(
      "d6b2cfdc39dae9bd3ad5b493ee1544eb22e7475f",
      source_code_hash("hi.ts", b"1+2", "0.2.0", b"{}")
    );
  }

  #[test]
  fn test_source_code_headers_get_and_save() {
    let (temp_dir, _deno_dir) = test_setup();
    let filename =
      deno_fs::normalize_path(temp_dir.into_path().join("f.js").as_ref());
    let headers_file_name = source_code_headers_filename(&filename);
    assert_eq!(headers_file_name, [&filename, ".headers.json"].concat());
    let _ = deno_fs::write_file(&PathBuf::from(&headers_file_name),
      "{\"mime_type\":\"text/javascript\",\"redirect_to\":\"http://example.com/a.js\"}", 0o666);
    let headers = get_source_code_headers(&filename);
    assert_eq!(headers.mime_type.clone().unwrap(), "text/javascript");
    assert_eq!(
      headers.redirect_to.clone().unwrap(),
      "http://example.com/a.js"
    );

    save_source_code_headers(
      &filename,
      Some("text/typescript".to_owned()),
      Some("http://deno.land/a.js".to_owned()),
    );
    let headers2 = get_source_code_headers(&filename);
    assert_eq!(headers2.mime_type.clone().unwrap(), "text/typescript");
    assert_eq!(
      headers2.redirect_to.clone().unwrap(),
      "http://deno.land/a.js"
    );
  }

  #[test]
  fn test_get_source_code_1() {
    let (_temp_dir, deno_dir) = test_setup();
    // http_util::fetch_sync_string requires tokio
    tokio_util::init(|| {
      let module_name = "http://localhost:4545/tests/subdir/mod2.ts";
      let filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/mod2.ts")
          .as_ref(),
      );
      let headers_file_name = source_code_headers_filename(&filename);

      let result =
        get_source_code(&deno_dir, module_name, &filename, true, false);
      assert!(result.is_ok());
      let r = result.unwrap();
      assert_eq!(
        r.source_code,
        "export { printHello } from \"./print_hello.ts\";\n".as_bytes()
      );
      assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
      // Should not create .headers.json file due to matching ext
      assert!(fs::read_to_string(&headers_file_name).is_err());

      // Modify .headers.json, write using fs write and read using save_source_code_headers
      let _ =
        fs::write(&headers_file_name, "{ \"mime_type\": \"text/javascript\" }");
      let result2 =
        get_source_code(&deno_dir, module_name, &filename, true, false);
      assert!(result2.is_ok());
      let r2 = result2.unwrap();
      assert_eq!(
        r2.source_code,
        "export { printHello } from \"./print_hello.ts\";\n".as_bytes()
      );
      // If get_source_code does not call remote, this should be JavaScript
      // as we modified before! (we do not overwrite .headers.json due to no http fetch)
      assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
      assert_eq!(
        get_source_code_headers(&filename).mime_type.unwrap(),
        "text/javascript"
      );

      // Modify .headers.json again, but the other way around
      save_source_code_headers(
        &filename,
        Some("application/json".to_owned()),
        None,
      );
      let result3 =
        get_source_code(&deno_dir, module_name, &filename, true, false);
      assert!(result3.is_ok());
      let r3 = result3.unwrap();
      assert_eq!(
        r3.source_code,
        "export { printHello } from \"./print_hello.ts\";\n".as_bytes()
      );
      // If get_source_code does not call remote, this should be JavaScript
      // as we modified before! (we do not overwrite .headers.json due to no http fetch)
      assert_eq!(&(r3.media_type), &msg::MediaType::Json);
      assert!(
        fs::read_to_string(&headers_file_name)
          .unwrap()
          .contains("application/json")
      );

      // Don't use_cache
      let result4 =
        get_source_code(&deno_dir, module_name, &filename, false, false);
      assert!(result4.is_ok());
      let r4 = result4.unwrap();
      let expected4 =
        "export { printHello } from \"./print_hello.ts\";\n".as_bytes();
      assert_eq!(r4.source_code, expected4);
      // Now the old .headers.json file should have gone! Resolved back to TypeScript
      assert_eq!(&(r4.media_type), &msg::MediaType::TypeScript);
      assert!(fs::read_to_string(&headers_file_name).is_err());
    });
  }

  #[test]
  fn test_get_source_code_2() {
    let (_temp_dir, deno_dir) = test_setup();
    // http_util::fetch_sync_string requires tokio
    tokio_util::init(|| {
      let module_name = "http://localhost:4545/tests/subdir/mismatch_ext.ts";
      let filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/mismatch_ext.ts")
          .as_ref(),
      );
      let headers_file_name = source_code_headers_filename(&filename);

      let result =
        get_source_code(&deno_dir, module_name, &filename, true, false);
      assert!(result.is_ok());
      let r = result.unwrap();
      let expected = "export const loaded = true;\n".as_bytes();
      assert_eq!(r.source_code, expected);
      // Mismatch ext with content type, create .headers.json
      assert_eq!(&(r.media_type), &msg::MediaType::JavaScript);
      assert_eq!(
        get_source_code_headers(&filename).mime_type.unwrap(),
        "text/javascript"
      );

      // Modify .headers.json
      save_source_code_headers(
        &filename,
        Some("text/typescript".to_owned()),
        None,
      );
      let result2 =
        get_source_code(&deno_dir, module_name, &filename, true, false);
      assert!(result2.is_ok());
      let r2 = result2.unwrap();
      let expected2 = "export const loaded = true;\n".as_bytes();
      assert_eq!(r2.source_code, expected2);
      // If get_source_code does not call remote, this should be TypeScript
      // as we modified before! (we do not overwrite .headers.json due to no http fetch)
      assert_eq!(&(r2.media_type), &msg::MediaType::TypeScript);
      assert!(fs::read_to_string(&headers_file_name).is_err());

      // Don't use_cache
      let result3 =
        get_source_code(&deno_dir, module_name, &filename, false, false);
      assert!(result3.is_ok());
      let r3 = result3.unwrap();
      let expected3 = "export const loaded = true;\n".as_bytes();
      assert_eq!(r3.source_code, expected3);
      // Now the old .headers.json file should be overwritten back to JavaScript!
      // (due to http fetch)
      assert_eq!(&(r3.media_type), &msg::MediaType::JavaScript);
      assert_eq!(
        get_source_code_headers(&filename).mime_type.unwrap(),
        "text/javascript"
      );
    });
  }

  #[test]
  fn test_get_source_code_3() {
    let (_temp_dir, deno_dir) = test_setup();
    // Test basic follow and headers recording
    tokio_util::init(|| {
      let redirect_module_name =
        "http://localhost:4546/tests/subdir/redirects/redirect1.js";
      let redirect_source_filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4546/tests/subdir/redirects/redirect1.js")
          .as_ref(),
      );
      let target_module_name =
        "http://localhost:4545/tests/subdir/redirects/redirect1.js";
      let redirect_target_filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/redirects/redirect1.js")
          .as_ref(),
      );
      let mod_meta = get_source_code(
        &deno_dir,
        redirect_module_name,
        &redirect_source_filename,
        true,
        false,
      ).unwrap();
      // File that requires redirection is not downloaded.
      assert!(fs::read_to_string(&redirect_source_filename).is_err());
      // ... but its .headers.json is created.
      let redirect_source_headers =
        get_source_code_headers(&redirect_source_filename);
      assert_eq!(
        redirect_source_headers.redirect_to.unwrap(),
        "http://localhost:4545/tests/subdir/redirects/redirect1.js"
      );
      // The target of redirection is downloaded instead.
      assert_eq!(
        fs::read_to_string(&redirect_target_filename).unwrap(),
        "export const redirect = 1;\n"
      );
      let redirect_target_headers =
        get_source_code_headers(&redirect_target_filename);
      assert!(redirect_target_headers.redirect_to.is_none());

      // Examine the meta result.
      assert_eq!(&mod_meta.module_name, target_module_name);
      assert_eq!(
        &mod_meta.module_redirect_source_name.clone().unwrap(),
        redirect_module_name
      );
    });
  }

  #[test]
  fn test_get_source_code_4() {
    let (_temp_dir, deno_dir) = test_setup();
    // Test double redirects and headers recording
    tokio_util::init(|| {
      let redirect_module_name =
        "http://localhost:4548/tests/subdir/redirects/redirect1.js";
      let redirect_source_filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4548/tests/subdir/redirects/redirect1.js")
          .as_ref(),
      );
      let redirect_source_filename_intermediate = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4546/tests/subdir/redirects/redirect1.js")
          .as_ref(),
      );
      let target_module_name =
        "http://localhost:4545/tests/subdir/redirects/redirect1.js";
      let redirect_target_filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/redirects/redirect1.js")
          .as_ref(),
      );
      let mod_meta = get_source_code(
        &deno_dir,
        redirect_module_name,
        &redirect_source_filename,
        true,
        false,
      ).unwrap();

      // File that requires redirection is not downloaded.
      assert!(fs::read_to_string(&redirect_source_filename).is_err());
      // ... but its .headers.json is created.
      let redirect_source_headers =
        get_source_code_headers(&redirect_source_filename);
      assert_eq!(
        redirect_source_headers.redirect_to.unwrap(),
        target_module_name
      );

      // In the intermediate redirection step, file is also not downloaded.
      assert!(
        fs::read_to_string(&redirect_source_filename_intermediate).is_err()
      );

      // The target of redirection is downloaded instead.
      assert_eq!(
        fs::read_to_string(&redirect_target_filename).unwrap(),
        "export const redirect = 1;\n"
      );
      let redirect_target_headers =
        get_source_code_headers(&redirect_target_filename);
      assert!(redirect_target_headers.redirect_to.is_none());

      // Examine the meta result.
      assert_eq!(&mod_meta.module_name, target_module_name);
      assert_eq!(
        &mod_meta.module_redirect_source_name.clone().unwrap(),
        redirect_module_name
      );
    });
  }

  #[test]
  fn test_get_source_code_no_fetch() {
    let (_temp_dir, deno_dir) = test_setup();
    tokio_util::init(|| {
      let module_name = "http://localhost:4545/tests/002_hello.ts";
      let filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/002_hello.ts")
          .as_ref(),
      );

      // file hasn't been cached before and remote downloads are not allowed
      let result =
        get_source_code(&deno_dir, module_name, &filename, true, true);
      assert!(result.is_err());
      let err = result.err().unwrap();
      assert_eq!(err.kind(), ErrorKind::NotFound);

      // download and cache file
      let result =
        get_source_code(&deno_dir, module_name, &filename, true, false);
      assert!(result.is_ok());

      // module is already cached, should be ok even with `no_fetch`
      let result =
        get_source_code(&deno_dir, module_name, &filename, true, true);
      assert!(result.is_ok());
    });
  }

  #[test]
  fn test_fetch_source_async_1() {
    use crate::tokio_util;
    // http_util::fetch_sync_string requires tokio
    tokio_util::init(|| {
      let (_temp_dir, deno_dir) = test_setup();
      let module_name =
        "http://127.0.0.1:4545/tests/subdir/mt_video_mp2t.t3.ts".to_string();
      let filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("127.0.0.1_PORT4545/tests/subdir/mt_video_mp2t.t3.ts")
          .as_ref(),
      );
      let headers_file_name = source_code_headers_filename(&filename);

      let result = tokio_util::block_on(fetch_remote_source_async(
        &deno_dir,
        &module_name,
        &filename,
      ));
      assert!(result.is_ok());
      let r = result.unwrap().unwrap();
      assert_eq!(r.source_code, b"export const loaded = true;\n");
      assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
      // matching ext, no .headers.json file created
      assert!(fs::read_to_string(&headers_file_name).is_err());

      // Modify .headers.json, make sure read from local
      save_source_code_headers(
        &filename,
        Some("text/javascript".to_owned()),
        None,
      );
      let result2 =
        fetch_local_source(&deno_dir, &module_name, &filename, None);
      assert!(result2.is_ok());
      let r2 = result2.unwrap().unwrap();
      assert_eq!(r2.source_code, b"export const loaded = true;\n");
      // Not MediaType::TypeScript due to .headers.json modification
      assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
    });
  }

  #[test]
  fn test_fetch_source_1() {
    use crate::tokio_util;
    // http_util::fetch_sync_string requires tokio
    tokio_util::init(|| {
      let (_temp_dir, deno_dir) = test_setup();
      let module_name =
        "http://localhost:4545/tests/subdir/mt_video_mp2t.t3.ts";
      let filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/mt_video_mp2t.t3.ts")
          .as_ref(),
      );
      let headers_file_name = source_code_headers_filename(&filename);

      let result = fetch_remote_source(&deno_dir, module_name, &filename);
      assert!(result.is_ok());
      let r = result.unwrap().unwrap();
      assert_eq!(r.source_code, "export const loaded = true;\n".as_bytes());
      assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
      // matching ext, no .headers.json file created
      assert!(fs::read_to_string(&headers_file_name).is_err());

      // Modify .headers.json, make sure read from local
      save_source_code_headers(
        &filename,
        Some("text/javascript".to_owned()),
        None,
      );
      let result2 = fetch_local_source(&deno_dir, module_name, &filename, None);
      assert!(result2.is_ok());
      let r2 = result2.unwrap().unwrap();
      assert_eq!(r2.source_code, "export const loaded = true;\n".as_bytes());
      // Not MediaType::TypeScript due to .headers.json modification
      assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
    });
  }

  #[test]
  fn test_fetch_source_2() {
    use crate::tokio_util;
    // http_util::fetch_sync_string requires tokio
    tokio_util::init(|| {
      let (_temp_dir, deno_dir) = test_setup();
      let module_name = "http://localhost:4545/tests/subdir/no_ext";
      let filename = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/no_ext")
          .as_ref(),
      );
      let result = fetch_remote_source(&deno_dir, module_name, &filename);
      assert!(result.is_ok());
      let r = result.unwrap().unwrap();
      assert_eq!(r.source_code, "export const loaded = true;\n".as_bytes());
      assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
      // no ext, should create .headers.json file
      assert_eq!(
        get_source_code_headers(&filename).mime_type.unwrap(),
        "text/typescript"
      );

      let module_name_2 = "http://localhost:4545/tests/subdir/mismatch_ext.ts";
      let filename_2 = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/mismatch_ext.ts")
          .as_ref(),
      );
      let result_2 = fetch_remote_source(&deno_dir, module_name_2, &filename_2);
      assert!(result_2.is_ok());
      let r2 = result_2.unwrap().unwrap();
      assert_eq!(r2.source_code, "export const loaded = true;\n".as_bytes());
      assert_eq!(&(r2.media_type), &msg::MediaType::JavaScript);
      // mismatch ext, should create .headers.json file
      assert_eq!(
        get_source_code_headers(&filename_2).mime_type.unwrap(),
        "text/javascript"
      );

      // test unknown extension
      let module_name_3 = "http://localhost:4545/tests/subdir/unknown_ext.deno";
      let filename_3 = deno_fs::normalize_path(
        deno_dir
          .deps_http
          .join("localhost_PORT4545/tests/subdir/unknown_ext.deno")
          .as_ref(),
      );
      let result_3 = fetch_remote_source(&deno_dir, module_name_3, &filename_3);
      assert!(result_3.is_ok());
      let r3 = result_3.unwrap().unwrap();
      assert_eq!(r3.source_code, "export const loaded = true;\n".as_bytes());
      assert_eq!(&(r3.media_type), &msg::MediaType::TypeScript);
      // unknown ext, should create .headers.json file
      assert_eq!(
        get_source_code_headers(&filename_3).mime_type.unwrap(),
        "text/typescript"
      );
    });
  }

  #[test]
  fn test_fetch_source_3() {
    // only local, no http_util::fetch_sync_string called
    let (_temp_dir, deno_dir) = test_setup();
    let cwd = std::env::current_dir().unwrap();
    let cwd_string = cwd.to_str().unwrap();
    let module_name = "http://example.com/mt_text_typescript.t1.ts"; // not used
    let filename =
      format!("{}/tests/subdir/mt_text_typescript.t1.ts", &cwd_string);

    let result = fetch_local_source(&deno_dir, module_name, &filename, None);
    assert!(result.is_ok());
    let r = result.unwrap().unwrap();
    assert_eq!(r.source_code, "export const loaded = true;\n".as_bytes());
    assert_eq!(&(r.media_type), &msg::MediaType::TypeScript);
  }

  #[test]
  fn test_fetch_module_meta_data() {
    let (_temp_dir, deno_dir) = test_setup();

    let cwd = std::env::current_dir().unwrap();
    let cwd_string = String::from(cwd.to_str().unwrap()) + "/";

    tokio_util::init(|| {
      // Test failure case.
      let specifier = "hello.ts";
      let referrer = add_root!("/baddir/badfile.ts");
      let r = deno_dir.fetch_module_meta_data(specifier, referrer, true, false);
      assert!(r.is_err());

      // Assuming cwd is the deno repo root.
      let specifier = "./js/main.ts";
      let referrer = cwd_string.as_str();
      let r = deno_dir.fetch_module_meta_data(specifier, referrer, true, false);
      assert!(r.is_ok());
    })
  }

  #[test]
  fn test_fetch_module_meta_data_1() {
    /*recompile ts file*/
    let (_temp_dir, deno_dir) = test_setup();

    let cwd = std::env::current_dir().unwrap();
    let cwd_string = String::from(cwd.to_str().unwrap()) + "/";

    tokio_util::init(|| {
      // Test failure case.
      let specifier = "hello.ts";
      let referrer = add_root!("/baddir/badfile.ts");
      let r =
        deno_dir.fetch_module_meta_data(specifier, referrer, false, false);
      assert!(r.is_err());

      // Assuming cwd is the deno repo root.
      let specifier = "./js/main.ts";
      let referrer = cwd_string.as_str();
      let r =
        deno_dir.fetch_module_meta_data(specifier, referrer, false, false);
      assert!(r.is_ok());
    })
  }

  #[test]
  fn test_src_file_to_url_1() {
    let (_temp_dir, deno_dir) = test_setup();
    assert_eq!("hello", deno_dir.src_file_to_url("hello"));
    assert_eq!("/hello", deno_dir.src_file_to_url("/hello"));
    let x = deno_dir.deps_http.join("hello/world.txt");
    assert_eq!(
      "http://hello/world.txt",
      deno_dir.src_file_to_url(x.to_str().unwrap())
    );
  }

  #[test]
  fn test_src_file_to_url_2() {
    let (_temp_dir, deno_dir) = test_setup();
    assert_eq!("hello", deno_dir.src_file_to_url("hello"));
    assert_eq!("/hello", deno_dir.src_file_to_url("/hello"));
    let x = deno_dir.deps_https.join("hello/world.txt");
    assert_eq!(
      "https://hello/world.txt",
      deno_dir.src_file_to_url(x.to_str().unwrap())
    );
  }

  #[test]
  fn test_src_file_to_url_3() {
    let (_temp_dir, deno_dir) = test_setup();
    let x = deno_dir.deps_http.join("localhost_PORT4545/world.txt");
    assert_eq!(
      "http://localhost:4545/world.txt",
      deno_dir.src_file_to_url(x.to_str().unwrap())
    );
  }

  #[test]
  fn test_src_file_to_url_4() {
    let (_temp_dir, deno_dir) = test_setup();
    let x = deno_dir.deps_https.join("localhost_PORT4545/world.txt");
    assert_eq!(
      "https://localhost:4545/world.txt",
      deno_dir.src_file_to_url(x.to_str().unwrap())
    );
  }

  // https://github.com/denoland/deno/blob/golang/os_test.go#L16-L87
  #[test]
  fn test_resolve_module_1() {
    let (_temp_dir, deno_dir) = test_setup();

    let test_cases = [
      (
        "./subdir/print_hello.ts",
        add_root!("/Users/rld/go/src/github.com/denoland/deno/testdata/006_url_imports.ts"),
        file_url!("/Users/rld/go/src/github.com/denoland/deno/testdata/subdir/print_hello.ts"),
        add_root!("/Users/rld/go/src/github.com/denoland/deno/testdata/subdir/print_hello.ts"),
      ),
      (
        "testdata/001_hello.js",
        add_root!("/Users/rld/go/src/github.com/denoland/deno/"),
        file_url!("/Users/rld/go/src/github.com/denoland/deno/testdata/001_hello.js"),
        add_root!("/Users/rld/go/src/github.com/denoland/deno/testdata/001_hello.js"),
      ),
      (
        add_root!("/Users/rld/src/deno/hello.js"),
        ".",
        file_url!("/Users/rld/src/deno/hello.js"),
        add_root!("/Users/rld/src/deno/hello.js"),
      ),
      (
        add_root!("/this/module/got/imported.js"),
        add_root!("/that/module/did/it.js"),
        file_url!("/this/module/got/imported.js"),
        add_root!("/this/module/got/imported.js"),
      ),
    ];
    for &test in test_cases.iter() {
      let specifier = String::from(test.0);
      let referrer = String::from(test.1);
      let (module_name, filename) =
        deno_dir.resolve_module(&specifier, &referrer).unwrap();
      assert_eq!(module_name, test.2);
      assert_eq!(filename, test.3);
    }
  }

  #[test]
  fn test_resolve_module_2() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "http://localhost:4545/testdata/subdir/print_hello.ts";
    let referrer = add_root!("/deno/testdata/006_url_imports.ts");

    let expected_module_name =
      "http://localhost:4545/testdata/subdir/print_hello.ts";
    let expected_filename = deno_fs::normalize_path(
      deno_dir
        .deps_http
        .join("localhost_PORT4545/testdata/subdir/print_hello.ts")
        .as_ref(),
    );

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_3() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier_ =
      deno_dir.deps_http.join("unpkg.com/liltest@0.0.5/index.ts");
    let specifier = specifier_.to_str().unwrap();
    let referrer = ".";

    let expected_module_name = "http://unpkg.com/liltest@0.0.5/index.ts";
    let expected_filename = deno_fs::normalize_path(
      deno_dir
        .deps_http
        .join("unpkg.com/liltest@0.0.5/index.ts")
        .as_ref(),
    );

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_4() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "./util";
    let referrer_ = deno_dir.deps_http.join("unpkg.com/liltest@0.0.5/index.ts");
    let referrer = referrer_.to_str().unwrap();

    // http containing files -> load relative import with http
    let expected_module_name = "http://unpkg.com/liltest@0.0.5/util";
    let expected_filename = deno_fs::normalize_path(
      deno_dir
        .deps_http
        .join("unpkg.com/liltest@0.0.5/util")
        .as_ref(),
    );

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_5() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "./util";
    let referrer_ =
      deno_dir.deps_https.join("unpkg.com/liltest@0.0.5/index.ts");
    let referrer = referrer_.to_str().unwrap();

    // https containing files -> load relative import with https
    let expected_module_name = "https://unpkg.com/liltest@0.0.5/util";
    let expected_filename = deno_fs::normalize_path(
      deno_dir
        .deps_https
        .join("unpkg.com/liltest@0.0.5/util")
        .as_ref(),
    );

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_6() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "http://localhost:4545/tests/subdir/mod2.ts";
    let referrer = add_root!("/deno/tests/006_url_imports.ts");
    let expected_module_name = "http://localhost:4545/tests/subdir/mod2.ts";
    let expected_filename = deno_fs::normalize_path(
      deno_dir
        .deps_http
        .join("localhost_PORT4545/tests/subdir/mod2.ts")
        .as_ref(),
    );

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_7() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "http_test.ts";
    let referrer = add_root!("/Users/rld/src/deno_net/");
    let expected_module_name =
      file_url!("/Users/rld/src/deno_net/http_test.ts");
    let expected_filename = add_root!("/Users/rld/src/deno_net/http_test.ts");

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_8() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "/util";
    let referrer_ =
      deno_dir.deps_https.join("unpkg.com/liltest@0.0.5/index.ts");
    let referrer = referrer_.to_str().unwrap();

    let expected_module_name = "https://unpkg.com/util";
    let expected_filename = deno_fs::normalize_path(
      deno_dir.deps_https.join("unpkg.com/util").as_ref(),
    );

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, referrer).unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_referrer_dot() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "tests/001_hello.js";

    let cwd = std::env::current_dir().unwrap();
    let expected_path = cwd.join(specifier);
    let expected_module_name =
      Url::from_file_path(&expected_path).unwrap().to_string();
    let expected_filename = deno_fs::normalize_path(&expected_path);

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, ".").unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, "./").unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_resolve_module_referrer_dotdot() {
    let (_temp_dir, deno_dir) = test_setup();

    let specifier = "tests/001_hello.js";

    let cwd = std::env::current_dir().unwrap();
    let expected_path = cwd.join("..").join(specifier);
    let expected_module_name =
      Url::from_file_path(&expected_path).unwrap().to_string();
    let expected_filename = deno_fs::normalize_path(&expected_path);

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, "..").unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);

    let (module_name, filename) =
      deno_dir.resolve_module(specifier, "../").unwrap();
    assert_eq!(module_name, expected_module_name);
    assert_eq!(filename, expected_filename);
  }

  #[test]
  fn test_map_file_extension() {
    assert_eq!(
      map_file_extension(Path::new("foo/bar.ts")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.d.ts")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.js")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.json")),
      msg::MediaType::Json
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar.txt")),
      msg::MediaType::Unknown
    );
    assert_eq!(
      map_file_extension(Path::new("foo/bar")),
      msg::MediaType::Unknown
    );
  }

  #[test]
  fn test_map_content_type() {
    // Extension only
    assert_eq!(
      map_content_type(Path::new("foo/bar.ts"), None),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.d.ts"), None),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.js"), None),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.json"), None),
      msg::MediaType::Json
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.txt"), None),
      msg::MediaType::Unknown
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), None),
      msg::MediaType::Unknown
    );

    // Media Type
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/typescript")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/typescript")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("video/vnd.dlna.mpeg-tts")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("video/mp2t")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/x-typescript")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/javascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/javascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/ecmascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/ecmascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/x-javascript")),
      msg::MediaType::JavaScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("application/json")),
      msg::MediaType::Json
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar"), Some("text/json")),
      msg::MediaType::Json
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.ts"), Some("text/plain")),
      msg::MediaType::TypeScript
    );
    assert_eq!(
      map_content_type(Path::new("foo/bar.ts"), Some("foo/bar")),
      msg::MediaType::Unknown
    );
  }

  #[test]
  fn test_filter_shebang() {
    assert_eq!(filter_shebang(b"#!"[..].to_owned()), b"");
    assert_eq!(
      filter_shebang("#!\n\n".as_bytes().to_owned()),
      "\n\n".as_bytes()
    );
    let code = "#!/usr/bin/env deno\nconsole.log('hello');\n"
      .as_bytes()
      .to_owned();
    assert_eq!(filter_shebang(code), "\nconsole.log('hello');\n".as_bytes());
  }
}
