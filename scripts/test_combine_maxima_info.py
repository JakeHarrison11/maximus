import gzip
import importlib.util
import pathlib
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "combine_maxima_info", pathlib.Path(__file__).with_name("combine-maxima-info.py")
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
combine = module.combine


class CombineMaximaInfoTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.source = pathlib.Path(self.temp.name)
        self.target = self.source / "help.info"

    def test_plain_and_compressed_chunks_are_sorted_numerically(self):
        (self.source / "maxima.info-10").write_bytes(b"tenth")
        (self.source / "maxima.info-1").write_bytes(b"Node: Top\n")
        with gzip.open(self.source / "maxima.info-2.gz", "wb") as stream:
            stream.write(b"second\n")
        (self.source / "maxima.info-3.gz.bak").write_bytes(b"not a chunk")
        (self.source / "maxima.info-4~").write_bytes(b"not a chunk")
        (self.source / "maxima.info-5").mkdir()
        combine(self.source, self.target)
        self.assertEqual(self.target.read_bytes(), b"Node: Top\nsecond\ntenth")

    def test_compressed_only_manual(self):
        with gzip.open(self.source / "maxima.info-1.gz", "wb") as stream:
            stream.write(b"Node: Top\n")
        combine(self.source, self.target)
        self.assertEqual(self.target.read_bytes(), b"Node: Top\n")

    def test_no_valid_chunks_fails_before_writing(self):
        (self.source / "maxima.info-1.gz.bak").write_bytes(b"backup")
        with self.assertRaisesRegex(ValueError, "No split English Maxima Info chunks"):
            combine(self.source, self.target)
        self.assertFalse(self.target.exists())

    def test_duplicate_chunks_fail_before_writing(self):
        (self.source / "maxima.info-1").write_bytes(b"plain")
        with gzip.open(self.source / "maxima.info-1.gz", "wb") as stream:
            stream.write(b"compressed")
        with self.assertRaisesRegex(ValueError, "Duplicate Maxima Info chunk 1"):
            combine(self.source, self.target)
        self.assertFalse(self.target.exists())

    def test_invalid_gzip_fails(self):
        (self.source / "maxima.info-1.gz").write_bytes(b"not gzip")
        with self.assertRaises(gzip.BadGzipFile):
            combine(self.source, self.target)


if __name__ == "__main__":
    unittest.main()
