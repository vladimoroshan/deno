# Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import os

from test_util import DenoTestCase, run_tests
from util import (shell_quote_win, root_path)


class TestUtil(DenoTestCase):
    def test_shell_quote_win(self):
        assert shell_quote_win('simple') == 'simple'
        assert shell_quote_win(
            'roof/\\isoprojection') == 'roof/\\isoprojection'
        assert shell_quote_win('with space') == '"with space"'
        assert shell_quote_win('embedded"quote') == '"embedded""quote"'
        assert shell_quote_win(
            'a"b""c\\d\\"e\\\\') == '"a""b""""c\\d\\\\""e\\\\\\\\"'

    def test_executable_exists(self):
        assert os.path.exists(self.deno_exe)


if __name__ == '__main__':
    run_tests()
