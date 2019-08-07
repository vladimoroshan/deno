// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
#include "test.h"

TEST(LibDenoTest, InitializesCorrectly) {
  EXPECT_NE(snapshot.data_ptr, nullptr);
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "1 + 2");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, Snapshotter) {
  Deno* d1 = deno_new(deno_config{1, empty_snapshot, empty, nullptr, nullptr});
  deno_execute(d1, nullptr, "a.js", "a = 1 + 2");
  EXPECT_EQ(nullptr, deno_last_exception(d1));
  deno_snapshot test_snapshot = deno_snapshot_new(d1);
  deno_delete(d1);

  Deno* d2 = deno_new(deno_config{0, test_snapshot, empty, nullptr, nullptr});
  deno_execute(d2, nullptr, "b.js", "if (a != 3) throw Error('x');");
  EXPECT_EQ(nullptr, deno_last_exception(d2));
  deno_delete(d2);

  deno_snapshot_delete(test_snapshot);
}

TEST(LibDenoTest, CanCallFunction) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_lock(d);
  deno_execute(d, nullptr, "a.js",
               "if (CanCallFunction() != 'foo') throw Error();");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_unlock(d);
  deno_delete(d);
}

TEST(LibDenoTest, ErrorsCorrectly) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "throw Error()");
  EXPECT_NE(nullptr, deno_last_exception(d));
  deno_delete(d);
}

void assert_null(deno_pinned_buf b) {
  EXPECT_EQ(b.data_ptr, nullptr);
  EXPECT_EQ(b.data_len, 0u);
  EXPECT_EQ(b.pin, nullptr);
}

TEST(LibDenoTest, RecvReturnEmpty) {
  static int count = 0;
  auto recv_cb = [](auto _, deno_op_id op_id, auto buf, auto zero_copy_buf) {
    EXPECT_EQ(op_id, 42u);
    assert_null(zero_copy_buf);
    count++;
    EXPECT_EQ(static_cast<size_t>(3), buf.data_len);
    EXPECT_EQ(buf.data_ptr[0], 'a');
    EXPECT_EQ(buf.data_ptr[1], 'b');
    EXPECT_EQ(buf.data_ptr[2], 'c');
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, nullptr});
  deno_execute(d, nullptr, "a.js", "RecvReturnEmpty()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(count, 2);
  deno_delete(d);
}

TEST(LibDenoTest, BasicRecv) {
  static int count = 0;
  auto recv_cb = [](auto user_data, deno_op_id op_id, auto buf,
                    auto zero_copy_buf) {
    EXPECT_EQ(op_id, 42u);
    // auto d = reinterpret_cast<Deno*>(user_data);
    assert_null(zero_copy_buf);
    count++;
    EXPECT_EQ(static_cast<size_t>(3), buf.data_len);
    EXPECT_EQ(buf.data_ptr[0], 1);
    EXPECT_EQ(buf.data_ptr[1], 2);
    EXPECT_EQ(buf.data_ptr[2], 3);
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, nullptr});
  deno_execute(d, d, "a.js", "BasicRecv()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(count, 1);
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);
  {
    deno_lock(d);
    uint8_t response[] = {'b', 'a', 'r'};
    deno_respond(d, nullptr, 43, {response, sizeof response});
    deno_unlock(d);
  }
  EXPECT_EQ(count, 2);
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_delete(d);
}

TEST(LibDenoTest, RecvReturnBar) {
  static int count = 0;
  auto recv_cb = [](auto user_data, deno_op_id op_id, auto buf,
                    auto zero_copy_buf) {
    EXPECT_EQ(op_id, 42u);
    auto d = reinterpret_cast<Deno*>(user_data);
    assert_null(zero_copy_buf);
    count++;
    EXPECT_EQ(static_cast<size_t>(3), buf.data_len);
    EXPECT_EQ(buf.data_ptr[0], 'a');
    EXPECT_EQ(buf.data_ptr[1], 'b');
    EXPECT_EQ(buf.data_ptr[2], 'c');
    uint8_t response[] = {'b', 'a', 'r'};
    deno_respond(d, user_data, op_id, {response, sizeof response});
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, nullptr});
  deno_execute(d, d, "a.js", "RecvReturnBar()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(count, 1);
  deno_delete(d);
}

TEST(LibDenoTest, DoubleRecvFails) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "DoubleRecvFails()");
  EXPECT_NE(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, TypedArraySnapshots) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "TypedArraySnapshots()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, SnapshotBug) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "SnapshotBug()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, GlobalErrorHandling) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "GlobalErrorHandling()");
  std::string expected =
      "{\"message\":\"Uncaught ReferenceError: notdefined is not defined\","
      "\"sourceLine\":\" "
      "notdefined()\",\"scriptResourceName\":\"helloworld.js\","
      "\"lineNumber\":3,\"startPosition\":3,\"endPosition\":4,\"errorLevel\":8,"
      "\"startColumn\":1,\"endColumn\":2,\"isSharedCrossOrigin\":false,"
      "\"isOpaque\":false,\"frames\":[{\"line\":3,\"column\":2,"
      "\"functionName\":\"eval\",\"scriptName\":\"helloworld.js\",\"isEval\":"
      "true,"
      "\"isConstructor\":false,\"isWasm\":false},";
  std::string actual(deno_last_exception(d), 0, expected.length());
  EXPECT_STREQ(expected.c_str(), actual.c_str());
  deno_delete(d);
}

TEST(LibDenoTest, ZeroCopyBuf) {
  static int count = 0;
  static deno_pinned_buf zero_copy_buf2;
  auto recv_cb = [](auto user_data, deno_op_id op_id, deno_buf buf,
                    deno_pinned_buf zero_copy_buf) {
    EXPECT_EQ(op_id, 42u);
    count++;
    EXPECT_NE(zero_copy_buf.pin, nullptr);
    zero_copy_buf.data_ptr[0] = 4;
    zero_copy_buf.data_ptr[1] = 2;
    zero_copy_buf2 = zero_copy_buf;
    EXPECT_EQ(2u, buf.data_len);
    EXPECT_EQ(2u, zero_copy_buf.data_len);
    EXPECT_EQ(buf.data_ptr[0], 1);
    EXPECT_EQ(buf.data_ptr[1], 2);
    // Note zero_copy_buf won't actually be freed here because in
    // libdeno_test.js zeroCopyBuf is a rooted global. We just want to exercise
    // the API here.
    deno_pinned_buf_delete(&zero_copy_buf);
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, nullptr});
  deno_execute(d, d, "a.js", "ZeroCopyBuf()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(count, 1);
  // zero_copy_buf was subsequently changed in JS, let's check that our copy
  // reflects that.
  EXPECT_EQ(zero_copy_buf2.data_ptr[0], 9);
  EXPECT_EQ(zero_copy_buf2.data_ptr[1], 8);
  deno_delete(d);
}

TEST(LibDenoTest, CheckPromiseErrors) {
  static int count = 0;
  auto recv_cb = [](auto _, deno_op_id op_id, auto buf, auto zero_copy_buf) {
    count++;
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, nullptr});
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_execute(d, nullptr, "a.js", "CheckPromiseErrors()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(count, 1);
  // We caught the exception. So still no errors after calling
  // deno_check_promise_errors().
  deno_check_promise_errors(d);
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_delete(d);
}

TEST(LibDenoTest, LastException) {
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, nullptr, nullptr});
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_execute(d, nullptr, "a.js", "\n\nthrow Error('boo');\n\n");
  EXPECT_STREQ(deno_last_exception(d),
               "{\"message\":\"Uncaught Error: boo\",\"sourceLine\":\"throw "
               "Error('boo');\",\"scriptResourceName\":\"a.js\",\"lineNumber\":"
               "3,\"startPosition\":8,\"endPosition\":9,\"errorLevel\":8,"
               "\"startColumn\":6,\"endColumn\":7,\"isSharedCrossOrigin\":"
               "false,\"isOpaque\":false,\"frames\":[{\"line\":3,\"column\":7,"
               "\"scriptName\":\"a.js\",\"isEval\":false,"
               "\"isConstructor\":false,\"isWasm\":false}]}");
  deno_delete(d);
}

TEST(LibDenoTest, EncodeErrorBug) {
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, nullptr, nullptr});
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_execute(d, nullptr, "a.js", "eval('a')");
  EXPECT_STREQ(
      deno_last_exception(d),
      "{\"message\":\"Uncaught ReferenceError: a is not "
      "defined\",\"sourceLine\":\"a\",\"lineNumber\":1,\"startPosition\":0,"
      "\"endPosition\":1,\"errorLevel\":8,\"startColumn\":0,\"endColumn\":1,"
      "\"isSharedCrossOrigin\":false,\"isOpaque\":false,\"frames\":[{\"line\":"
      "1,\"column\":1,\"functionName\":\"eval\",\"scriptName\":\"<unknown>\","
      "\"isEval\":true,\"isConstructor\":false,\"isWasm\":false},{\"line\":1,"
      "\"column\":1,\"scriptName\":\"a.js\",\"isEval\":"
      "false,\"isConstructor\":false,\"isWasm\":false}]}");
  deno_delete(d);
}

TEST(LibDenoTest, Shared) {
  uint8_t s[] = {0, 1, 2};
  deno_buf shared = {s, sizeof s};
  Deno* d = deno_new(deno_config{0, snapshot, shared, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "Shared()");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(s[0], 42);
  EXPECT_EQ(s[1], 43);
  EXPECT_EQ(s[2], 44);
  deno_delete(d);
}

TEST(LibDenoTest, Utf8Bug) {
  Deno* d = deno_new(deno_config{0, empty_snapshot, empty, nullptr, nullptr});
  // The following is a valid UTF-8 javascript which just defines a string
  // literal. We had a bug where libdeno would choke on this.
  deno_execute(d, nullptr, "a.js", "x = \"\xEF\xBF\xBD\"");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, LibDenoEvalContext) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "LibDenoEvalContext();");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, LibDenoEvalContextError) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "LibDenoEvalContextError();");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, LibDenoEvalContextInvalidArgument) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "LibDenoEvalContextInvalidArgument();");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, LibDenoPrintInvalidArgument) {
  Deno* d = deno_new(deno_config{0, snapshot, empty, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js", "LibDenoPrintInvalidArgument();");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_delete(d);
}

TEST(LibDenoTest, SharedAtomics) {
  int32_t s[] = {0, 1, 2};
  deno_buf shared = {reinterpret_cast<uint8_t*>(s), sizeof s};
  Deno* d = deno_new(deno_config{0, empty_snapshot, shared, nullptr, nullptr});
  deno_execute(d, nullptr, "a.js",
               "Atomics.add(new Int32Array(Deno.core.shared), 0, 1)");
  EXPECT_EQ(nullptr, deno_last_exception(d));
  EXPECT_EQ(s[0], 1);
  EXPECT_EQ(s[1], 1);
  EXPECT_EQ(s[2], 2);
  deno_delete(d);
}

TEST(LibDenoTest, WasmInstantiate) {
  static int count = 0;
  auto recv_cb = [](auto _, deno_op_id op_id, auto buf, auto zero_copy_buf) {
    EXPECT_EQ(op_id, 42u);
    EXPECT_EQ(buf.data_len, 1u);
    EXPECT_EQ(buf.data_ptr[0], 42);
    count++;
  };
  Deno* d = deno_new(deno_config{0, snapshot, empty, recv_cb, nullptr});
  EXPECT_EQ(deno_last_exception(d), nullptr);
  deno_execute(d, nullptr, "a.js", "WasmInstantiate()");

  EXPECT_EQ(nullptr, deno_last_exception(d));
  deno_check_promise_errors(d);
  EXPECT_EQ(nullptr, deno_last_exception(d));

  EXPECT_EQ(count, 3);

  deno_delete(d);
}
