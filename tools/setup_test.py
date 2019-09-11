# Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import os
import sys
from shutil import rmtree
from tempfile import mktemp
from setup import gn_string, read_gn_args, write_gn_args
from test_util import DenoTestCase, run_tests


class TestSetup(DenoTestCase):
    def test_gn_string(self):
        assert gn_string('abc') == '"abc"'
        assert gn_string('foo$bar"baz') == '"foo\\$bar\\"baz"'
        assert gn_string('do\\not\\escape') == '"do\\not\\escape"'
        assert gn_string('so\\\\very\\"fun\\') == '"so\\\\\\very\\\\\\"fun\\"'

    def test_read_gn_args(self):
        # Args file doesn't exist.
        (args,
         hand_edited) = read_gn_args("/baddir/hopefully/nonexistent/args.gn")
        assert args is None
        assert not hand_edited

        # Handwritten empty args file.
        filename = mktemp()
        with open(filename, "w"):
            pass
        (args, hand_edited) = read_gn_args(filename)
        os.remove(filename)
        assert args == []
        assert hand_edited

        # Handwritten non-empty args file.
        expect_args = ['some_number=2', 'another_string="ran/dom#yes"']
        filename = mktemp()
        with open(filename, "w") as f:
            f.write("\n".join(expect_args + ["", "# A comment to be ignored"]))
        (args, hand_edited) = read_gn_args(filename)
        os.remove(filename)
        assert args == expect_args
        assert hand_edited

    def test_write_gn_args(self):
        # Build a nonexistent path; write_gn_args() should call mkdir as needed.
        d = mktemp()
        filename = os.path.join(d, "args.gn")
        assert not os.path.exists(d)
        assert not os.path.exists(filename)
        # Write some args.
        args = ['lalala=42', 'foo_bar_baz="lorem ipsum dolor#amet"']
        write_gn_args(filename, args)
        # Directory and args file should now be created.
        assert os.path.isdir(d)
        assert os.path.isfile(filename)
        # Validate that the right contents were written.
        (check_args, hand_edited) = read_gn_args(filename)
        assert check_args == args
        assert not hand_edited
        # Clean up.
        rmtree(d)


if __name__ == '__main__':
    run_tests()
