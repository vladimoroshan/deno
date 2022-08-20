import { fromFileUrl } from "../../../../test_util/std/path/mod.ts";

const DENO_NODE_COMPAT_URL = Deno.env.get("DENO_NODE_COMPAT_URL");
const moduleAllUrl = `${DENO_NODE_COMPAT_URL}node/module_all.ts`;
let moduleName = import.meta.resolve(Deno.args[0]);
moduleName = fromFileUrl(moduleName);

const moduleAll = await import(moduleAllUrl);
Deno[Deno.internal].node.initialize(moduleAll.default);
Deno[Deno.internal].require.Module._load(moduleName, null, true);
