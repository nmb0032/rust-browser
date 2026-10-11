"""Offline integrity and failure-mode tests for CSS spec generation."""

import copy
import unittest
from unittest import mock

import generate_css_properties as generator


class GenerationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data, cls.engine, cls.provenance = generator.load()

    def test_complete_snapshot_counts_and_supported_expansion_gaps(self):
        properties, longhands, supported = generator.engine_registry(self.data, self.engine)
        self.assertEqual(len(properties), 821)
        self.assertEqual(sum("legacyAliasOf" in p for p in properties.values()), 64)
        self.assertEqual(len(longhands), 50)
        self.assertEqual(len(supported), 69)
        self.assertNotIn("direction", supported["all"]["targets"])
        self.assertEqual(supported["-webkit-box-sizing"]["targets"], ["box-sizing"])
        self.assertNotIn("initial", properties["-webkit-box-sizing"])
        self.assertEqual(supported["margin"]["targets"],
                         ["margin-top", "margin-right", "margin-bottom", "margin-left"])
        self.assertEqual(supported["container"]["targets"], ["container-name", "container-type"])
        self.assertEqual(supported["white-space"]["targets"], ["white-space-collapse", "text-wrap-mode"])
        self.assertNotIn("white-space-trim", supported)
        self.assertEqual(supported["inset"]["targets"], ["top", "right", "bottom", "left"])
        self.assertNotIn("inset-inline-start", supported)
        self.assertEqual(supported["background"]["targets"],
                         ["background-color", "background-image"])
        self.assertIn("font-variant-ligatures", supported["font"]["missing"])
        self.assertIn("border-image-source", supported["border"]["missing_reset"])
        self.assertIn("background-blend-mode", supported["background"]["missing_reset"])

    def test_unknown_or_missing_supported_properties_fail(self):
        engine = copy.deepcopy(self.engine)
        engine["longhands"]["unknown-engine-property"] = {
            "rust": "Unknown", "policy": "size", "sample": "1px"}
        with self.assertRaisesRegex(ValueError, "missing from snapshot"):
            generator.engine_registry(self.data, engine)
        data = copy.deepcopy(self.data)
        data["properties"] = [p for p in data["properties"] if p["name"] != "height"]
        with self.assertRaisesRegex(ValueError, "missing from snapshot: height"):
            generator.engine_registry(data, self.engine)

    def test_missing_spec_fields_cannot_be_filled_in_by_engine_claims(self):
        for field in ("syntax", "initial", "inherited", "computedValue", "animationType"):
            data = copy.deepcopy(self.data)
            next(p for p in data["properties"] if p["name"] == "font-size").pop(field)
            with self.assertRaisesRegex(ValueError, f"missing {field}: font-size"):
                generator.engine_registry(data, self.engine)

    def test_alias_duplicate_and_keyword_mismatches_fail(self):
        engine = copy.deepcopy(self.engine)
        engine["aliases"]["-webkit-box-sizing"] = "width"
        with self.assertRaisesRegex(ValueError, "alias differs from spec"):
            generator.engine_registry(self.data, engine)
        engine = copy.deepcopy(self.engine)
        engine["aliases"]["-webkit-flex"] = "flex"
        with self.assertRaisesRegex(ValueError, "no canonical handler"):
            generator.engine_registry(self.data, engine)
        engine = copy.deepcopy(self.engine)
        engine["longhands"]["font-stretch"] = {
            "rust": "Stretch", "policy": "size", "sample": "normal"}
        with self.assertRaisesRegex(ValueError, "alias/shorthand"):
            generator.engine_registry(self.data, engine)
        engine = copy.deepcopy(self.engine)
        engine["longhands"]["height"]["rust"] = "Width"
        with self.assertRaisesRegex(ValueError, "duplicate engine Rust property"):
            generator.engine_registry(self.data, engine)
        engine = copy.deepcopy(self.engine)
        engine["simple_values"]["display"]["keywords"]["invented-display"] = "Value::Display(Display::Block)"
        with self.assertRaisesRegex(ValueError, "keywords missing from spec"):
            generator.engine_registry(self.data, engine)
        engine = copy.deepcopy(self.engine)
        engine["shorthands"]["background"]["order"] = ["background-color"]
        with self.assertRaisesRegex(ValueError, "ordering differs from spec"):
            generator.engine_registry(self.data, engine)

    def test_duplicate_snapshot_names_and_inheritance_ambiguity_fail(self):
        data = copy.deepcopy(self.data)
        data["properties"].append(data["properties"][0])
        with self.assertRaisesRegex(ValueError, "duplicate property"):
            generator.engine_registry(data, self.engine)
        data = copy.deepcopy(self.data)
        next(p for p in data["properties"] if p["name"] == "content")["inherited"] = "unknown"
        with self.assertRaisesRegex(ValueError, "non-boolean longhand inheritance"):
            generator.engine_registry(data, self.engine)

    def test_outputs_are_deterministic_and_regeneration_never_needs_network(self):
        with mock.patch.object(generator.urllib.request, "urlopen", side_effect=AssertionError("network")):
            first = generator.generate(self.data, self.engine, self.provenance)
            second = generator.generate(self.data, self.engine, self.provenance)
            self.assertEqual(first, second)
            self.assertEqual(first[0], generator.RUST.read_bytes())
            self.assertEqual(first[1], generator.REPORT.read_bytes())

    def test_pinned_refresh_rejects_bad_artifact_before_writing(self):
        with mock.patch.object(generator.urllib.request, "urlopen") as request:
            request.return_value.read.return_value = b"not-the-pinned-webref-artifact"
            with self.assertRaisesRegex(ValueError, "tarball hash mismatch"):
                generator.refresh(self.provenance)


if __name__ == "__main__":
    unittest.main()
