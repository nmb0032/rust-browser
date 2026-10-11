"""Generate finite engine dispatch and honest coverage from pinned Webref CSS.

Normal generation and --check are offline. --refresh downloads only the exact
artifact and license identified by provenance.json and verifies their hashes.
The engine overlay describes implementation, never substitutes for spec data.
"""

import argparse
import hashlib
import io
import json
import pathlib
import re
import subprocess
import sys
import tarfile
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "tools/css-spec"
RUST = ROOT / "src/style/generated_properties.rs"
REPORT = ROOT / "docs/css-coverage.md"
SIDES = ("top", "right", "bottom", "left")
ANIMATION_GAP = "No animation engine (including discrete animation)."
UNIMPLEMENTED = "No executable property parser, cascade, computed or used-value handler."


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def refresh(provenance):
    # No archive extraction: read only one explicitly named regular member.
    data = urllib.request.urlopen(provenance["url"], timeout=60).read()
    if (sha256(data) != provenance["tarball_sha256"]
            or hashlib.sha512(data).hexdigest() != provenance["tarball_sha512"]):
        raise ValueError("Webref tarball hash mismatch")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        member = archive.getmember(provenance["member"])
        if not member.isfile() or member.size > 4_000_000:
            raise ValueError("unexpected Webref snapshot member")
        snapshot = archive.extractfile(member).read()
    license_data = urllib.request.urlopen(provenance["license_url"], timeout=60).read()
    if sha256(snapshot) != provenance["snapshot_sha256"]:
        raise ValueError("Webref snapshot hash mismatch")
    if sha256(license_data) != provenance["license_sha256"]:
        raise ValueError("Webref license hash mismatch")
    (SOURCE / "webref-css.json").write_bytes(snapshot)
    (SOURCE / "LICENSE").write_bytes(license_data)


def load():
    provenance = read_json(SOURCE / "provenance.json")
    raw = (SOURCE / "webref-css.json").read_bytes()
    if sha256(raw) != provenance["snapshot_sha256"]:
        raise ValueError("snapshot hash mismatch; review provenance when updating")
    if sha256((SOURCE / "LICENSE").read_bytes()) != provenance["license_sha256"]:
        raise ValueError("license hash mismatch")
    return json.loads(raw), read_json(SOURCE / "engine-support.json"), provenance


def index(entries, category):
    result = {}
    for entry in entries:
        name = entry["name"]
        if name in result:
            raise ValueError(f"duplicate {category}: {name}")
        result[name] = entry
    return result


def spec_keywords(syntax, properties, types, visited=None):
    """Collect reachable literal terminals, not a value-definition parser.

    Used only to check that finite engine keyword mappings exist in the spec.
    It deliberately does not claim that every reachable terminal is a valid
    standalone value (e.g. display's multi-keyword productions).
    """
    visited = set() if visited is None else visited
    keywords = set()
    for reference in re.findall(r"<([^>]+)>", syntax):
        name = reference.split(" [", 1)[0]
        table = properties if name.startswith("'") else types
        name = name.strip("'")
        key = (id(table), name)
        if key not in visited and name in table:
            visited.add(key)
            keywords |= spec_keywords(table[name].get("syntax", ""), properties, types, visited)
    terminals = re.sub(r"<[^>]+>", "", syntax)
    keywords |= {word.lower() for word in re.findall(r"[a-zA-Z][a-zA-Z0-9-]*", terminals)}
    return keywords


def flatten(name, properties, visiting=None):
    visiting = set() if visiting is None else visiting
    if name not in properties:
        raise ValueError(f"missing referenced property: {name}")
    if name in visiting:
        raise ValueError(f"cyclic shorthand/alias: {name}")
    entry = properties[name]
    targets = entry.get("longhands")
    if entry.get("legacyAliasOf"):
        targets = [entry["legacyAliasOf"]]
    if not targets:
        return [name]
    result = []
    for target in targets:
        for child in flatten(target, properties, visiting | {name}):
            if child not in result:
                result.append(child)
    return result


def engine_registry(data, engine):
    properties = index(data["properties"], "property")
    # Some types have scope-specific definitions. They are preserved in output;
    # keyword validation may use the union of those definitions.
    types = {}
    for entry in data["types"]:
        types.setdefault(entry["name"], {"syntax": ""})
        types[entry["name"]]["syntax"] += " | " + entry.get("syntax", "")
    longhands = dict(engine["longhands"])
    for pattern, details in engine["side_families"].items():
        for side in SIDES:
            name = pattern.replace("*", side) if "*" in pattern else f"{pattern}-{side}"
            if name in longhands:
                raise ValueError(f"duplicate engine longhand: {name}")
            longhands[name] = {**details, "rust": f'{details["rust"]}(Side::{side.title()})'}
    supported = {name: {**details, "parser": "Longhand", "targets": [name]}
                 for name, details in longhands.items()}
    rust_ids = set()
    for name, details in longhands.items():
        if name not in properties:
            raise ValueError(f"engine property missing from snapshot: {name}")
        prop = properties[name]
        if prop.get("legacyAliasOf") or prop.get("longhands"):
            raise ValueError(f"engine longhand is an alias/shorthand: {name}")
        for field in ("syntax", "initial", "inherited", "computedValue", "animationType"):
            if field not in prop:
                raise ValueError(f"supported property missing {field}: {name}")
        if prop["inherited"] not in ("yes", "no"):
            raise ValueError(f"non-boolean longhand inheritance: {name}")
        if details["rust"] in rust_ids:
            raise ValueError(f"duplicate engine Rust property: {details['rust']}")
        rust_ids.add(details["rust"])
        if details["policy"] not in engine["policies"]:
            raise ValueError(f"missing support policy: {name}")
        simple = engine["simple_values"].get(details["policy"])
        if simple and simple.get("color") and "<color>" not in (
                part.strip() for part in prop["syntax"].split("|")):
            raise ValueError(f"color dispatch requires a top-level <color> alternative: {name}")
        if simple and "keywords" in simple:
            unknown = set(simple["keywords"]) - spec_keywords(prop["syntax"], properties, types)
            if unknown:
                raise ValueError(f"engine keywords missing from spec: {name}: {sorted(unknown)}")
        if "ua_initial" in details and prop["initial"] != "depends on user agent":
            raise ValueError(f"UA initial override on fixed spec initial: {name}")
    for name, details in engine["shorthands"].items():
        if name not in properties:
            raise ValueError(f"engine shorthand missing from snapshot: {name}")
        if name == "all":
            # CSS Cascade excludes direction/unicode-bidi and custom properties.
            targets = [n for n in longhands if n not in ("direction", "unicode-bidi")]
            missing = []
        else:
            if not properties[name].get("longhands"):
                raise ValueError(f"engine shorthand lacks spec longhands: {name}")
            expanded = flatten(name, properties)
            targets = [target for target in expanded if target in longhands]
            missing = [target for target in expanded if target not in longhands]
        if details.get("side_major"):
            ordered = [f"border-{side}-{part}" for side in SIDES
                       for part in ("width", "style", "color")]
            if set(ordered) != set(targets):
                raise ValueError(f"side-major expansion differs from spec: {name}")
            targets = ordered
        if "order" in details:
            if set(details["order"]) != set(targets) or len(details["order"]) != len(targets):
                raise ValueError(f"engine shorthand ordering differs from spec: {name}")
            targets = details["order"]
        reset = [child for target in properties[name].get("resetLonghands", [])
                 for child in flatten(target, properties)]
        if not targets:
            raise ValueError(f"shorthand has no implemented targets: {name}")
        if details["parser"] == "Four" and len(targets) != 4:
            raise ValueError(f"four-value shorthand does not have four targets: {name}")
        supported[name] = {**details, "targets": targets, "missing": missing,
                           "missing_reset": [n for n in reset if n not in longhands]}
    for name, target in engine.get("aliases", {}).items():
        if name not in properties or properties[name].get("legacyAliasOf") != target:
            raise ValueError(f"engine alias differs from spec: {name} -> {target}")
        if target not in supported or "alias_of" in supported[target]:
            raise ValueError(f"engine alias target has no canonical handler: {name} -> {target}")
        supported[name] = {**supported[target], "alias_of": target}
    return properties, longhands, supported


def rs(value):
    return '"' + "".join(f"\\u{{{ord(c):x}}}" if ord(c) < 32 else
                        "\\\\" if c == "\\" else '\\"' if c == '"' else c for c in value) + '"'


def optional(value):
    return "None" if value is None else f"Some({rs(value)})"


def strings(values):
    return "&[" + ", ".join(rs(value) for value in values) + "]"


def syntax_record(entry):
    return ("SyntaxMetadata { " + f'name: {rs(entry["name"])}, href: {rs(entry["href"])}, '
            + f'scopes: {strings(entry.get("for", []) if isinstance(entry.get("for", []), list) else [entry["for"]])}, '
            + f'syntax: {optional(entry.get("syntax"))}, initial: {optional(entry.get("initial"))}' + " }")


def generate(data, engine, provenance):
    properties, longhands, supported = engine_registry(data, engine)
    lines = [
        "// Generated by tools/generate_css_properties.py; do not edit.",
        f'// @webref/css {provenance["version"]}, MIT; see tools/css-spec/LICENSE and provenance.json.',
        f'// Snapshot SHA-256: {provenance["snapshot_sha256"]}',
        "use super::{declarations::{Property, Value}, metadata::{Grammar, PropertyMetadata, Support, SyntaxMetadata}, BaselineSource, BoxSizing, Direction, Display, FlexDirection, FlexWrap, Position, Overflow, TextAlign, TextAlignLast, TextWrapMode, VerticalAlign, WhiteSpaceCollapse, Float, Clear, TableLayout, BorderCollapse, CaptionSide, EmptyCells};",
        "use crate::css::{container::ContainerType, values::{BackgroundImage, BorderStyle, FontStyle, Side, components::Component}};",
        "pub static PROPERTIES: &[PropertyMetadata] = &[",
    ]
    for name, prop in sorted(properties.items()):
        details = supported.get(name)
        status = "Partial" if details else (
            "AliasUnimplemented" if prop.get("legacyAliasOf") else "Unimplemented")
        simple = engine["simple_values"].get(details["policy"]) if details and details["parser"] == "Longhand" else None
        grammar = "Grammar::Unsupported" if not details else (
            "Grammar::ColorSubset" if simple and simple.get("color") else
            f'Grammar::Keywords({strings(sorted(simple["keywords"]))})' if simple else
            "Grammar::SemanticSubset")
        inherited = {"yes": "Some(true)", "no": "Some(false)"}.get(prop.get("inherited"), "None")
        lines.append("PropertyMetadata { " + ", ".join([
            f"name: {rs(name)}", f'href: {rs(prop["href"])}',
            f'syntax: {optional(prop.get("syntax"))}', f'initial: {optional(prop.get("initial"))}',
            f'inheritance: {optional(prop.get("inherited"))}', f"inherited: {inherited}",
            f'computed_value: {optional(prop.get("computedValue"))}',
            f'animation_type: {optional(prop.get("animationType"))}',
            f'alias_of: {optional(prop.get("legacyAliasOf"))}',
            f'longhands: {strings(prop.get("longhands", []))}',
            f'reset_longhands: {strings(prop.get("resetLonghands", []))}',
            f"grammar: {grammar}", f"support: Support::{status}"]) + " },")
    lines.extend(["];", "pub static TYPES: &[SyntaxMetadata] = &["])
    for entry in sorted(data["types"], key=lambda e: (e["name"], e.get("for", []))):
        lines.append(syntax_record(entry) + ",")
    lines.extend(["];", "pub static FUNCTIONS: &[SyntaxMetadata] = &["])
    for entry in sorted(data["functions"], key=lambda e: (e["name"], e.get("for", []))):
        lines.append(syntax_record(entry) + ",")
    lines.extend(["];", "pub static PROPERTY_DESCRIPTORS: &[SyntaxMetadata] = &["])
    rule = next(entry for entry in data["atrules"] if entry["name"] == "@property")
    for entry in sorted(rule["descriptors"], key=lambda e: e["name"]):
        lines.append(syntax_record(entry) + ",")
    lines.extend([
        "];", "#[derive(Clone, Copy)]",
        "pub(super) enum Parser { Longhand, Wide, Four, Border, Font, Background, Container, TextAlign, VerticalAlign, WhiteSpace, Overflow, Flex, FlexFlow, Gap, GridPlacement, Place }",
        "pub(super) fn parser(name: &str) -> Option<Parser> { Some(match name {",
    ])
    for name, details in sorted(supported.items()):
        lines.append(f'{rs(name)} => Parser::{details["parser"]},')
    lines.extend(["_ => return None,", "}) }",
                  "pub(super) fn properties(name: &str) -> Option<Vec<Property>> { Some(match name {"])
    for name, details in sorted(supported.items()):
        values = ", ".join("Property::" + longhands[target]["rust"] for target in details["targets"])
        lines.append(f"{rs(name)} => vec![{values}],")
    lines.extend(["_ => return None,", "}) }", "impl Property {",
                  "pub(super) fn name(self) -> &'static str { match self {"])
    for name, details in longhands.items():
        lines.append(f'Self::{details["rust"]} => {rs(name)},')
    lines.extend(["} }", "pub(super) fn inherited(self) -> bool { match self {"])
    for name, details in longhands.items():
        lines.append(f'Self::{details["rust"]} => {str(properties[name]["inherited"] == "yes").lower()},')
    lines.extend(["} }", "pub(super) fn computes_absolute_length(self) -> bool { match self {"])
    for name, details in longhands.items():
        absolute = properties[name]["computedValue"] == "an absolute length"
        lines.append(f'Self::{details["rust"]} => {str(absolute).lower()},')
    lines.extend(["} }", "}",
                  "pub(super) fn initial_css(property: Property) -> &'static str { match property {"])
    for name, details in longhands.items():
        value = details.get("ua_initial", properties[name]["initial"])
        lines.append(f'Property::{details["rust"]} => {rs(value)},')
    lines.extend(["} }", "pub(super) fn simple(property: Property) -> bool { matches!(property,"])
    simple_names = [name for name, details in longhands.items()
                    if details["policy"] in engine["simple_values"]]
    lines.append(" | ".join("Property::" + longhands[n]["rust"] for n in simple_names))
    lines.extend([") }", "pub(super) fn simple_value(property: Property, tokens: &[Component]) -> Option<Value> {",
                  "let [token] = tokens else { return None; };", "match property {"])
    for policy, values in engine["simple_values"].items():
        names = [name for name in simple_names if longhands[name]["policy"] == policy]
        patterns = " | ".join("Property::" + longhands[n]["rust"] for n in names)
        if values.get("color"):
            lines.append(f"{patterns} => token.color().map(Value::Color),")
        else:
            lines.append(f"{patterns} => Some(match token.ident()?.to_ascii_lowercase().as_str() {{")
            for token, value in sorted(values["keywords"].items()):
                lines.append(f"{rs(token)} => {value},")
            lines.extend(["_ => return None,", "}),"])
    lines.extend(["_ => None,", "} }",
                  "#[cfg(test)] pub(super) static SAMPLES: &[(&str, &str)] = &["])
    for name, details in sorted(supported.items()):
        lines.append(f'({rs(name)}, {rs(details["sample"])}),')
    lines.append("];")
    process = subprocess.run(["rustfmt", "--edition", "2024"], input="\n".join(lines) + "\n",
                             text=True, capture_output=True, check=True)
    return process.stdout.encode(), report(properties, supported, engine, provenance).encode()


def cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def report(properties, supported, engine, provenance):
    aliases = sum("legacyAliasOf" in p for p in properties.values())
    canonical = len(properties) - aliases
    accepted_aliases = sum("alias_of" in details for details in supported.values())
    partial = len(supported) - accepted_aliases
    result = [
        "# Generated CSS coverage", "",
        f'Source: [`@webref/css {provenance["version"]}`]({provenance["url"]}), MIT.',
        f'Repository ref: `{provenance["ref"]}`.',
        f'Snapshot SHA-256: `{provenance["snapshot_sha256"]}`.', "",
        "Regenerate offline: `python3 tools/generate_css_properties.py`; verify: "
        "`python3 tools/generate_css_properties.py --check`.",
        "Read without loading a page: `cargo run -- --dump=css-coverage`.", "",
        f"Snapshot entries: **{len(properties)}**; canonical properties: **{canonical}**; "
        f"legacy aliases: **{aliases}**.",
        f"Canonical support: **0 implemented**, **{partial} partial**, "
        f"**{canonical - partial} unimplemented**. Legacy aliases: "
        f"**{accepted_aliases} accepted (partial targets)**, **{aliases - accepted_aliases} unimplemented** (not independent grammar conformance).",
        f"Engine surface: **{sum(d['parser'] == 'Longhand' and 'alias_of' not in d for d in supported.values())} longhands**, "
        f"**{sum(d['parser'] != 'Longhand' and 'alias_of' not in d for d in supported.values())} shorthands**.", "",
        "Partial means an executable finite engine handler exists, not that an AST accepts "
        "the name or that its spec grammar/computation/layout is complete. Implemented would "
        "require all stages and a verified applicable external conformance gate. No property "
        "currently meets that gate. All property animation remains absent. No WPT or "
        "Chrome/Firefox parity result is asserted.", "",
        "Full syntax, initial, inheritance, computed-value and animation strings are generated "
        "as metadata (missing source fields remain None). Only the color/keyword subsets and "
        "dispatch to existing semantic handlers are executable; arbitrary prose and the full "
        "value-definition grammar are **not** executable implementations. The exact spec "
        "`an absolute length` rule selects the existing font-size absolute-length stage; "
        "other contextual computations remain manual. Type/function productions "
        "and @property descriptors are exposed separately. This is the complete bounded "
        "release snapshot, including experimental/SVG/compat properties, not a Baseline list "
        "or a claim about what browsers ship.", "",
        "## Partial property stage gaps", "",
        "| Property | Executable dispatch | Parser gap / finite scope | Cascade gap | "
        "Computed-value gap | Used/layout/paint gap | Animation gap |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for name, details in sorted(supported.items()):
        gaps = engine["policies"][details["policy"]]
        cascade = gaps["cascade"]
        if details.get("missing"):
            cascade += " Missing expansion targets: " + ", ".join(details["missing"]) + "."
        if details.get("missing_reset"):
            cascade += " Missing reset targets: " + ", ".join(details["missing_reset"]) + "."
        route = "semantic subset: " + details["parser"]
        if details.get("alias_of"):
            route = "legacy name alias of " + details["alias_of"]
        simple = engine["simple_values"].get(details["policy"]) if details["parser"] == "Longhand" else None
        if simple and not details.get("alias_of"):
            route = "color subset" if simple.get("color") else (
                "generated keywords: " + ", ".join(sorted(simple["keywords"])))
        result.append("| " + " | ".join(map(cell, [name, route, gaps["parser"], cascade,
                                                   gaps["computed"], gaps["used"], ANIMATION_GAP])) + " |")
    result.extend([
        "", "## Custom properties and registration (outside the property denominator)", "",
        "Webref does not enumerate arbitrary `--name` properties. Unregistered case-sensitive "
        "custom properties inherit bounded computed token streams with a guaranteed-invalid "
        "initial value. The runtime @property registration's inherits/initial-value descriptors "
        "override those rules. The generated descriptor syntax/default metadata does not supply "
        "missing required descriptors: the existing registration validator still requires "
        "syntax and inherits, and typed syntax requires a computationally independent initial-value.",
        "Only `*`, `<length>`, `<number>`, `<integer>`, `<percentage>`, `<color>` registration "
        "syntax is executable; no alternatives/lists, relative registered lengths/currentcolor "
        "dependency model, full CSSOM or animation. This is **partial**, not another implemented "
        "snapshot property.", "",
        "## All snapshot properties", "",
        "Unsupported metadata does not grant parser acceptance. Only explicitly registered "
        "aliases with supported canonical targets are accepted. Their source metadata "
        "(including missing initial/inheritance fields) is preserved, not inferred from targets.", "",
        "| Property | Kind | Support | Spec syntax | Initial | Inheritance | Computed value | "
        "Spec animation type |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ])
    for name, prop in sorted(properties.items()):
        kind = "alias of " + prop["legacyAliasOf"] if "legacyAliasOf" in prop else (
            "shorthand" if prop.get("longhands") or name == "all" else "longhand")
        support = "partial" if name in supported else (
            "alias unimplemented" if "legacyAliasOf" in prop else "unimplemented")
        fields = [name, kind, support] + [prop.get(field, "(missing in snapshot)")
                                         for field in ("syntax", "initial", "inherited", "computedValue", "animationType")]
        result.append("| " + " | ".join(map(cell, fields)) + " |")
    result.extend(["", "All unimplemented rows: " + UNIMPLEMENTED + " " + ANIMATION_GAP, ""])
    return "\n".join(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail on stale output; never write")
    parser.add_argument("--refresh", action="store_true", help="redownload the pinned, hash-checked public source")
    args = parser.parse_args()
    if args.check and args.refresh:
        parser.error("--check is offline and cannot be combined with --refresh")
    if args.refresh:
        refresh(read_json(SOURCE / "provenance.json"))
    outputs = generate(*load())
    stale = []
    for path, content in zip((RUST, REPORT), outputs):
        if args.check:
            if not path.exists() or path.read_bytes() != content:
                stale.append(str(path.relative_to(ROOT)))
        else:
            path.write_bytes(content)
    if stale:
        print("stale generated CSS output: " + ", ".join(stale), file=sys.stderr)
        return 1
    print("CSS generation " + ("verified" if args.check else "complete"))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"CSS generation failed: {error}", file=sys.stderr)
        sys.exit(1)
