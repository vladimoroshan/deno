// Copyright 2018 the Deno authors. All rights reserved. MIT license.

export async function getJson(path) {
  return (await fetch(path)).json();
}

const benchmarkNames = [
  "hello",
  "relative_import",
  "cold_hello",
  "cold_relative_import"
];
export function createExecTimeColumns(data) {
  return benchmarkNames.map(name => [
    name,
    ...data.map(d => {
      const benchmark = d.benchmark[name];
      const meanValue = benchmark ? benchmark.mean : 0;
      return meanValue || 0;
    })
  ]);
}

const binarySizeNames = ["deno", "main.js", "main.js.map", "snapshot_deno.bin"];
export function createBinarySizeColumns(data) {
  return binarySizeNames.map(name => [
    name,
    ...data.map(d => {
      const binarySizeData = d["binary_size"];
      switch (typeof binarySizeData) {
        case "number": // legacy implementation
          return name === "deno" ? binarySizeData : 0;
        default:
          if (!binarySizeData) {
            return 0;
          }
          return binarySizeData[name] || 0;
      }
    })
  ]);
}

const threadCountNames = ["set_timeout", "fetch_deps"];
export function createThreadCountColumns(data) {
  return threadCountNames.map(name => [
    name,
    ...data.map(d => {
      const threadCountData = d["thread_count"];
      if (!threadCountData) {
        return 0;
      }
      return threadCountData[name] || 0;
    })
  ]);
}

const syscallCountNames = ["hello"];
export function createSyscallCountColumns(data) {
  return syscallCountNames.map(name => [
    name,
    ...data.map(d => {
      const syscallCountData = d["syscall_count"];
      if (!syscallCountData) {
        return 0;
      }
      return syscallCountData[name] || 0;
    })
  ]);
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

export async function main() {
  const data = await getJson("./data.json");

  const execTimeColumns = createExecTimeColumns(data);
  const binarySizeColumns = createBinarySizeColumns(data);
  const threadCountColumns = createThreadCountColumns(data);
  const syscallCountColumns = createSyscallCountColumns(data);
  const sha1List = createSha1List(data);

  c3.generate({
    bindto: "#exec-time-chart",
    data: { columns: execTimeColumns },
    axis: {
      x: {
        type: "category",
        categories: sha1List
      }
    }
  });

  c3.generate({
    bindto: "#binary-size-chart",
    data: { columns: binarySizeColumns },
    axis: {
      x: {
        type: "category",
        categories: sha1List
      },
      y: {
        tick: {
          format: d => formatBytes(d)
        }
      }
    }
  });

  c3.generate({
    bindto: "#thread-count-chart",
    data: { columns: threadCountColumns },
    axis: {
      x: {
        type: "category",
        categories: sha1List
      }
    }
  });

  c3.generate({
    bindto: "#syscall-count-chart",
    data: { columns: syscallCountColumns },
    axis: {
      x: {
        type: "category",
        categories: sha1List
      }
    }
  });
}
