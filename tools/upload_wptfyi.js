// This script pushes new WPT results to wpt.fyi. When the `--ghstatus` flag is
// passed, will automatically add a status check to the commit with a link to
// the wpt.fyi page.

const user = Deno.env.get("WPT_FYI_STAGING_USER");
const password = Deno.env.get("WPT_FYI_STAGING_PW");

const commit = Deno.args[0];

const form = new FormData();
form.set("labels", "experimental");
form.set("result_url", `https://dl.deno.land/wpt/${commit}-wptreport.json.gz`);

const basicAuthToken = btoa(`${user}:${password}`);

const resp = await fetch("https://staging.wpt.fyi/api/results/upload", {
  method: "POST",
  body: form,
  headers: {
    authorization: `Basic ${basicAuthToken}`,
  },
});

console.log(resp.status);
console.log(resp.headers);
const body = await resp.text();
console.log(body);

if (!resp.ok) {
  Deno.exit(1);
}

if (Deno.args.includes("--ghstatus")) {
  const githubToken = Deno.env.get("GITHUB_TOKEN");
  const taskId = body.split(" ")[1];
  const url = `https://staging.wpt.fyi/results/?run_id=${taskId}`;
  const resp = await fetch(
    `https://api.github.com/repos/denoland/deno/statuses/${commit}`,
    {
      method: "POST",
      body: JSON.stringify({
        state: "success",
        target_url: url,
        context: "wpt.fyi",
        description: "View WPT results on wpt.fyi",
      }),
      headers: {
        authorization: `Bearer ${githubToken}`,
      },
    },
  );
  console.log(resp.status);
  console.log(resp.headers);
  const body2 = await resp.text();
  console.log(body2);

  if (!resp.ok) {
    Deno.exit(1);
  }
}
