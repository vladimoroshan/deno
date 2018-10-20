// Copyright 2018 the Deno authors. All rights reserved. MIT license.

export async function getJson(path) {
  return (await fetch(path)).json();
}

export async function getTravisData(
  url = "https://api.travis-ci.com/repos/denoland/deno/builds?event_type=pull_request"
) {
  const res = await fetch(url, {
    headers: {
      Accept: "application/vnd.travis-ci.2.1+json"
    }
  });
  const data = await res.json();
  return data.builds.reverse();
}

function getBenchmarkVarieties(data, benchmarkName) {
  // Look at last sha hash.
  const last = data[data.length - 1];
  return Object.keys(last[benchmarkName]);
}

export function createColumns(data, benchmarkName) {
  const varieties = getBenchmarkVarieties(data, benchmarkName);
  return varieties.map(variety => [
    variety,
    ...data.map(d => {
      if (d[benchmarkName] != null) {
        if (d[benchmarkName][variety] != null) {
          const v = d[benchmarkName][variety];
          if (benchmarkName == "benchmark") {
            const meanValue = v ? v.mean : 0;
            return meanValue || null;
          } else {
            return v;
          }
        }
      }
      return null;
    })
  ]);
}

export function createExecTimeColumns(data) {
  return createColumns(data, "benchmark");
}

export function createThroughputColumns(data) {
  return createColumns(data, "throughput");
}

export function createReqPerSecColumns(data) {
  return createColumns(data, "req_per_sec");
}

export function createBinarySizeColumns(data) {
  const propName = "binary_size";
  const binarySizeNames = Object.keys(data[data.length - 1][propName]);
  return binarySizeNames.map(name => [
    name,
    ...data.map(d => {
      const binarySizeData = d["binary_size"];
      switch (typeof binarySizeData) {
        case "number": // legacy implementation
          return name === "deno" ? binarySizeData : 0;
        default:
          if (!binarySizeData) {
            return null;
          }
          return binarySizeData[name] || null;
      }
    })
  ]);
}

export function createThreadCountColumns(data) {
  const propName = "thread_count";
  const threadCountNames = Object.keys(data[data.length - 1][propName]);
  return threadCountNames.map(name => [
    name,
    ...data.map(d => {
      const threadCountData = d[propName];
      if (!threadCountData) {
        return null;
      }
      return threadCountData[name] || null;
    })
  ]);
}

export function createSyscallCountColumns(data) {
  const propName = "syscall_count";
  const syscallCountNames = Object.keys(data[data.length - 1][propName]);
  return syscallCountNames.map(name => [
    name,
    ...data.map(d => {
      const syscallCountData = d[propName];
      if (!syscallCountData) {
        return null;
      }
      return syscallCountData[name] || null;
    })
  ]);
}

function createTravisCompileTimeColumns(data) {
  return [["duration_time", ...data.map(d => d.duration)]];
}

export function createSha1List(data) {
  return data.map(d => d.sha1);
}

// Formats the byte sizes e.g. 19000 -> 18.55 KB
// Copied from https://stackoverflow.com/a/18650828
export function formatBytes(a, b) {
  if (0 == a) return "0 Bytes";
  var c = 1024,
    d = b || 2,
    e = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"],
    f = Math.floor(Math.log(a) / Math.log(c));
  return parseFloat((a / Math.pow(c, f)).toFixed(d)) + " " + e[f];
}

/**
 * @param {string} id The id of dom element
 * @param {any[][]} columns The columns data
 * @param {string[]} categories The sha1 hashes (which work as x-axis values)
 */
function gen2(id, categories, columns, onclick) {
  c3.generate({
    bindto: id,
    size: {
      height: 300,
      width: window.chartWidth || 375 // TODO: do not use global variable
    },
    data: {
      columns,
      onclick
    },
    axis: {
      x: {
        type: "category",
        show: false,
        categories
      },
      y: {
        label: "seconds"
      }
    }
  });
}

export function formatSeconds(t) {
  const a = t % 60;
  const min = Math.floor(t / 60);
  return a < 30 ? `${min} min` : `${min + 1} min`;
}

/**
 * @param dataUrl The url of benchramk data json.
 */
export function drawCharts(dataUrl) {
  drawChartsFromBenchmarkData(dataUrl);
  drawChartsFromTravisData();
}

/**
 * Draws the charts from the benchmark data stored in gh-pages branch.
 */
export async function drawChartsFromBenchmarkData(dataUrl) {
  const data = await getJson(dataUrl);

  const execTimeColumns = createExecTimeColumns(data);
  const throughputColumns = createThroughputColumns(data);
  const reqPerSecColumns = createReqPerSecColumns(data);
  const binarySizeColumns = createBinarySizeColumns(data);
  const threadCountColumns = createThreadCountColumns(data);
  const syscallCountColumns = createSyscallCountColumns(data);
  const sha1List = createSha1List(data);
  const sha1ShortList = sha1List.map(sha1 => sha1.substring(0, 6));

  const viewCommitOnClick = _sha1List => d => {
    window.open(
      `https://github.com/denoland/deno/commit/${_sha1List[d["index"]]}`
    );
  };

  function gen(id, columns) {
    gen2(id, sha1ShortList, columns, viewCommitOnClick(sha1List));
  }

  gen("#exec-time-chart", execTimeColumns);
  gen("#throughput-chart", throughputColumns);
  gen("#req-per-sec-chart", reqPerSecColumns);

  /* TODO 
    axis: {
      y: {
        tick: {
          format: d => formatBytes(d)
        }
      }
    }
  */
  gen("#binary-size-chart", binarySizeColumns);
  gen("#thread-count-chart", threadCountColumns);
  gen("#syscall-count-chart", syscallCountColumns);
}

/**
 * Draws the charts from travis' API data.
 */
export async function drawChartsFromTravisData() {
  const viewPullRequestOnClick = _prNumberList => d => {
    window.open(
      `https://github.com/denoland/deno/pull/${_prNumberList[d["index"]]}`
    );
  };

  const travisData = (await getTravisData()).filter(d => d.duration > 0);
  const travisCompileTimeColumns = createTravisCompileTimeColumns(travisData);
  const prNumberList = travisData.map(d => d.pull_request_number);

  gen2(
    "#travis-compile-time-chart",
    prNumberList,
    travisCompileTimeColumns,
    viewPullRequestOnClick(prNumberList)
  );
}
