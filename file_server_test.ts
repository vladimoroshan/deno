import { readFile } from "deno";

import {
  test,
  assert,
  assertEqual
} from "https://deno.land/x/testing/testing.ts";

// Promise to completeResolve when all tests completes
let completeResolve;
export const completePromise = new Promise(res => (completeResolve = res));
let completedTestCount = 0;

function maybeCompleteTests() {
  completedTestCount++;
  // Change this when adding more tests
  if (completedTestCount === 3) {
    completeResolve();
  }
}

export function runTests(serverReadyPromise: Promise<any>) {
  test(async function serveFile() {
    await serverReadyPromise;
    const res = await fetch("http://localhost:4500/.travis.yml");
    const downloadedFile = await res.text();
    const localFile = new TextDecoder().decode(await readFile("./.travis.yml"));
    assertEqual(downloadedFile, localFile);
    maybeCompleteTests();
  });

  test(async function serveDirectory() {
    await serverReadyPromise;
    const res = await fetch("http://localhost:4500/");
    const page = await res.text();
    assert(page.includes(".travis.yml"));
    maybeCompleteTests();
  });

  test(async function serveFallback() {
    await serverReadyPromise;
    const res = await fetch("http://localhost:4500/badfile.txt");
    assertEqual(res.status, 404);
    maybeCompleteTests();
  });
}
