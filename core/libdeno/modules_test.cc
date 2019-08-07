// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
#include "test.h"

static int exec_count = 0;
void recv_cb(void* user_data, deno_op_id op_id, deno_buf buf,
             deno_pinned_buf zero_copy_buf) {
  // We use this to check that scripts have executed.
  EXPECT_EQ(1u, buf.data_len);
  EXPECT_EQ(42u, op_id);
  EXPECT_EQ(buf.data_ptr[0], 4);
  EXPECT_EQ(zero_copy_buf.data_ptr, nullptr);
  EXPECT_EQ(zero_copy_buf.data_len, 0u);
  EXPECT_EQ(zero_copy_buf.pin, nullptr);
  exec_count++;
}

TEST(ModulesTest, Resolution) {
  exec_count = 0;  // Reset
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, recv_cb, nullptr});
  EXPECT_EQ(0, exec_count);

  static deno_mod a = deno_mod_new(d, true, "a.js",
                                   "import { b } from 'b.js'\n"
                                   "if (b() != 'b') throw Error();\n"
                                   "Deno.core.send(42, new Uint8Array([4]));");
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  const char* b_src = "export function b() { return 'b' }";
  static deno_mod b = deno_mod_new(d, false, "b.js", b_src);
  EXPECT_NE(b, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  EXPECT_EQ(0, exec_count);

  EXPECT_EQ(1u, deno_mod_imports_len(d, a));
  EXPECT_EQ(0u, deno_mod_imports_len(d, b));

  EXPECT_STREQ("b.js", deno_mod_imports_get(d, a, 0));
  EXPECT_EQ(nullptr, deno_mod_imports_get(d, a, 1));
  EXPECT_EQ(nullptr, deno_mod_imports_get(d, b, 0));

  static int resolve_count = 0;
  auto resolve_cb = [](void* user_data, const char* specifier,
                       deno_mod referrer) {
    EXPECT_EQ(referrer, a);
    EXPECT_STREQ(specifier, "b.js");
    resolve_count++;
    return b;
  };

  deno_mod_instantiate(d, d, b, resolve_cb);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(0, resolve_count);
  EXPECT_EQ(0, exec_count);

  deno_mod_instantiate(d, d, a, resolve_cb);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(1, resolve_count);
  EXPECT_EQ(0, exec_count);

  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(1, resolve_count);
  EXPECT_EQ(1, exec_count);

  deno_delete(d);
}

TEST(ModulesTest, ResolutionError) {
  exec_count = 0;  // Reset
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, recv_cb, nullptr});
  EXPECT_EQ(0, exec_count);

  static deno_mod a = deno_mod_new(d, true, "a.js",
                                   "import 'bad'\n"
                                   "Deno.core.send(42, new Uint8Array([4]));");
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  EXPECT_EQ(0, exec_count);

  EXPECT_EQ(1u, deno_mod_imports_len(d, a));
  EXPECT_STREQ("bad", deno_mod_imports_get(d, a, 0));

  static int resolve_count = 0;
  auto resolve_cb = [](void* user_data, const char* specifier,
                       deno_mod referrer) {
    EXPECT_EQ(referrer, a);
    EXPECT_STREQ(specifier, "bad");
    resolve_count++;
    return 0;
  };

  deno_mod_instantiate(d, d, a, resolve_cb);
  EXPECT_NE(nullptr, deno_last_exception(d));
  EXPECT_EQ(1, resolve_count);
  EXPECT_EQ(0, exec_count);

  deno_delete(d);
}

TEST(ModulesTest, ImportMetaUrl) {
  exec_count = 0;  // Reset
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, recv_cb, nullptr});
  EXPECT_EQ(0, exec_count);

  static deno_mod a =
      deno_mod_new(d, true, "a.js",
                   "if ('a.js' != import.meta.url) throw 'hmm'\n"
                   "Deno.core.send(42, new Uint8Array([4]));");
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  deno_mod_instantiate(d, d, a, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(0, exec_count);

  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(1, exec_count);
}

TEST(ModulesTest, ImportMetaMain) {
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, recv_cb, nullptr});

  const char* throw_not_main_src = "if (!import.meta.main) throw 'err'";
  static deno_mod throw_not_main =
      deno_mod_new(d, true, "a.js", throw_not_main_src);
  EXPECT_NE(throw_not_main, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  deno_mod_instantiate(d, d, throw_not_main, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  deno_mod_evaluate(d, d, throw_not_main);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  const char* throw_main_src = "if (import.meta.main) throw 'err'";
  static deno_mod throw_main = deno_mod_new(d, false, "b.js", throw_main_src);
  EXPECT_NE(throw_main, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  deno_mod_instantiate(d, d, throw_main, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  deno_mod_evaluate(d, d, throw_main);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  deno_delete(d);
}

TEST(ModulesTest, DynamicImportSuccess) {
  exec_count = 0;
  static int dyn_import_count = 0;
  static deno_mod b = 0;
  auto dyn_import_cb = [](auto user_data, const char* specifier,
                          const char* referrer, deno_dyn_import_id import_id) {
    auto d = reinterpret_cast<Deno*>(user_data);
    dyn_import_count++;
    EXPECT_STREQ(specifier, "foo");
    EXPECT_STREQ(referrer, "a.js");
    deno_dyn_import_done(d, d, import_id, b, nullptr);
  };
  const char* src =
      "(async () => { \n"
      "  let mod = await import('foo'); \n"
      "  assert(mod.b() === 'b'); \n"
      // Send a message to signify that we're done.
      "  Deno.core.send(42, new Uint8Array([4])); \n"
      "})(); \n";
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, dyn_import_cb});
  static deno_mod a = deno_mod_new(d, true, "a.js", src);
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, a, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  const char* b_src = "export function b() { return 'b' }";
  b = deno_mod_new(d, false, "b.js", b_src);
  EXPECT_NE(b, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, b, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_evaluate(d, d, b);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_delete(d);
  EXPECT_EQ(1, exec_count);
  EXPECT_EQ(1, dyn_import_count);
}

TEST(ModulesTest, DynamicImportError) {
  exec_count = 0;
  static int dyn_import_count = 0;
  auto dyn_import_cb = [](auto user_data, const char* specifier,
                          const char* referrer, deno_dyn_import_id import_id) {
    auto d = reinterpret_cast<Deno*>(user_data);
    dyn_import_count++;
    EXPECT_STREQ(specifier, "foo");
    EXPECT_STREQ(referrer, "a.js");
    // We indicate there was an error resolving by returning mod_id 0.
    deno_dyn_import_done(d, d, import_id, 0, "foo not found");
  };
  const char* src =
      "(async () => { \n"
      "  let mod = await import('foo'); \n"
      // The following should be unreachable.
      "  Deno.core.send(42, new Uint8Array([4])); \n"
      "})(); \n";
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, dyn_import_cb});
  static deno_mod a = deno_mod_new(d, true, "a.js", src);
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, a, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  // No error when evaluating, because it's an async error.
  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  // Now we should get an error.
  deno_check_promise_errors(d);
  EXPECT_NE(deno_last_exception(d), nullptr);
  std::string e(deno_last_exception(d));
  EXPECT_NE(e.find("Uncaught TypeError: foo not found"), std::string::npos);
  deno_delete(d);
  EXPECT_EQ(0, exec_count);
  EXPECT_EQ(1, dyn_import_count);
}

TEST(ModulesTest, DynamicImportAsync) {
  exec_count = 0;
  static int dyn_import_count = 0;
  static deno_mod b = 0;
  static std::vector<deno_dyn_import_id> import_ids = {};
  auto dyn_import_cb = [](auto user_data, const char* specifier,
                          const char* referrer, deno_dyn_import_id import_id) {
    // auto d = reinterpret_cast<Deno*>(user_data);
    dyn_import_count++;
    EXPECT_STREQ(specifier, "foo");
    EXPECT_STREQ(referrer, "a.js");
    // We don't call deno_dyn_import_done until later.
    import_ids.push_back(import_id);
  };
  const char* src =
      "(async () => { \n"
      "  let mod = await import('foo'); \n"
      "  assert(mod.b() === 'b'); \n"
      // AGAIN!
      "  mod = await import('foo'); \n"
      "  assert(mod.b() === 'b'); \n"
      // Send a message to signify that we're done.
      "  Deno.core.send(42, new Uint8Array([4])); \n"
      "})(); \n";
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, dyn_import_cb});
  static deno_mod a = deno_mod_new(d, true, "a.js", src);
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, a, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  // Evaluate. We check that there are no errors, and Deno.core.send has not
  // been called.
  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);
  EXPECT_EQ(0, exec_count);
  EXPECT_EQ(1, dyn_import_count);

  // Instantiate b.js
  const char* b_src = "export function b() { return 'b' }";
  b = deno_mod_new(d, false, "b.js", b_src);
  EXPECT_NE(b, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, b, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_evaluate(d, d, b);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  // Now we resolve the import.
  EXPECT_EQ(1u, import_ids.size());
  auto import_id = import_ids.back();
  import_ids.pop_back();

  deno_dyn_import_done(d, d, import_id, b, nullptr);

  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);

  EXPECT_EQ(1u, import_ids.size());
  EXPECT_EQ(2, dyn_import_count);
  EXPECT_EQ(0, exec_count);

  import_id = import_ids.back();
  import_ids.pop_back();
  deno_dyn_import_done(d, d, import_id, b, nullptr);

  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);

  // We still have to resolve the second one
  EXPECT_EQ(2, dyn_import_count);
  EXPECT_EQ(1, exec_count);

  deno_delete(d);
}

TEST(ModulesTest, DynamicImportThrows) {
  exec_count = 0;
  static int dyn_import_count = 0;
  static deno_mod b = 0;
  static std::vector<deno_dyn_import_id> import_ids = {};
  auto dyn_import_cb = [](auto user_data, const char* specifier,
                          const char* referrer, deno_dyn_import_id import_id) {
    dyn_import_count++;
    EXPECT_STREQ(specifier, "b.js");
    EXPECT_STREQ(referrer, "a.js");
    // We don't call deno_dyn_import_done until later.
    import_ids.push_back(import_id);
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, dyn_import_cb});

  // Instantiate and evaluate the root module. This should succeed.
  const char* a_src =
      "(async () => { \n"
      "  let mod = await import('b.js'); \n"
      // unreachable
      "  Deno.core.send(42, new Uint8Array([4])); \n"
      "})(); \n";
  static deno_mod a = deno_mod_new(d, true, "a.js", a_src);
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, a, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);

  // Instantiate b.js, which should succeed.
  const char* b_src = "throw new Error('foo')";
  b = deno_mod_new(d, false, "b.js", b_src);
  EXPECT_NE(b, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, b, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  // Evaluate b.js. It throws in the global scope, so deno_last_exception()
  // should be non-null afterwards.
  deno_mod_evaluate(d, d, b);
  EXPECT_NE(deno_last_exception(d), nullptr);

  // Resolve the dynamic import of b.js. Since deno_mod_evaluate() failed,
  // we indicate failure to deno_dyn_import_done() by setting mod_id to 0.
  // The last error should be picked up and cleared by deno_dyn_import_done().
  EXPECT_EQ(1u, import_ids.size());
  auto import_id = import_ids.back();
  import_ids.pop_back();
  deno_dyn_import_done(d, d, import_id, 0, nullptr);
  EXPECT_EQ(deno_last_exception(d), nullptr);

  // Since the dynamically imported module threw an error,
  // it should show up as an unhandled promise rejection.
  deno_check_promise_errors(d);
  EXPECT_NE(deno_last_exception(d), nullptr);
  std::string e(deno_last_exception(d));
  EXPECT_NE(e.find("Uncaught Error: foo"), std::string::npos);

  EXPECT_EQ(1, dyn_import_count);
  EXPECT_EQ(0, exec_count);

  deno_delete(d);
}

TEST(ModulesTest, DynamicImportSyntaxError) {
  exec_count = 0;
  static int dyn_import_count = 0;
  auto dyn_import_cb = [](auto user_data, const char* specifier,
                          const char* referrer, deno_dyn_import_id import_id) {
    auto d = reinterpret_cast<Deno*>(user_data);
    dyn_import_count++;
    EXPECT_STREQ(specifier, "b.js");
    EXPECT_STREQ(referrer, "a.js");

    // Compile b.js, which should fail because of the syntax error.
    deno_mod b = deno_mod_new(d, false, "b.js", "syntax error");
    EXPECT_EQ(b, 0);
    EXPECT_NE(nullptr, deno_last_exception(d));

    // `deno_dyn_import_done` should consume the last exception, and use it
    // to reject the dynamic import promise.
    deno_dyn_import_done(d, d, import_id, 0, nullptr);
    EXPECT_EQ(nullptr, deno_last_exception(d));
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, dyn_import_cb});

  // Instantiate and evaluate the root module. This should succeed.
  const char* src =
      "(async () => { \n"
      "  let mod = await import('b.js'); \n"
      // unreachable
      "  Deno.core.send(42, new Uint8Array([4])); \n"
      "})(); \n";
  static deno_mod a = deno_mod_new(d, true, "a.js", src);
  EXPECT_NE(a, 0);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_instantiate(d, d, a, nullptr);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_mod_evaluate(d, d, a);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  // The failed dynamic import should cause an unhandled promise rejection.
  deno_check_promise_errors(d);
  EXPECT_NE(deno_last_exception(d), nullptr);
  EXPECT_NE(std::string(deno_last_exception(d)).find("Syntax"),
            std::string::npos);

  EXPECT_EQ(1, dyn_import_count);
  EXPECT_EQ(0, exec_count);

  deno_delete(d);
}
