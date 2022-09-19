try {
  Deno.removeSync("./lock_write_fetch.json");
} catch {
  // pass
}

const fetchProc = await Deno.spawn(Deno.execPath(), {
  stdout: "null",
  stderr: "null",
  args: [
    "cache",
    "--reload",
    "--lock=lock_write_fetch.json",
    "--lock-write",
    "--cert=tls/RootCA.pem",
    "run/https_import.ts",
  ],
});

console.log(`fetch code: ${fetchProc.code}`);

const fetchCheckProc = await Deno.spawn(Deno.execPath(), {
  stdout: "null",
  stderr: "null",
  args: [
    "cache",
    "--lock=lock_write_fetch.json",
    "--cert=tls/RootCA.pem",
    "run/https_import.ts",
  ],
});

console.log(`fetch check code: ${fetchCheckProc.code}`);

Deno.removeSync("./lock_write_fetch.json");

const runProc = await Deno.spawn(Deno.execPath(), {
  stdout: "null",
  stderr: "null",
  args: [
    "run",
    "--lock=lock_write_fetch.json",
    "--lock-write",
    "--allow-read",
    "run/lock_write_fetch/file_exists.ts",
    "lock_write_fetch.json",
  ],
});

console.log(`run code: ${runProc.code}`);

Deno.removeSync("./lock_write_fetch.json");
