#!/usr/bin/env python3
"""Fail closed when mechanically-verifiable design contracts drift.

This gate deliberately verifies only constraints that can be established from
repository sources without rendering a UI.  It is not a substitute for the
accessibility, content, or screenshot review described in DESIGN.md.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "web"
TOKEN_STYLESHEET = WEB / "design-system" / "src" / "styles.css"
IMAGE_MANIFEST = ROOT / "docs" / "IMAGE2_ASSET_MANIFEST.zh-CN.md"
ILLUSTRATION_DIRECTORY = WEB / "design-system" / "assets" / "illustrations"

IMAGE_SUFFIXES = frozenset({".png", ".webp", ".avif"})
DELIVERY_IMAGE_SUFFIXES = frozenset({".webp", ".avif"})
SOURCE_EXTENSIONS = frozenset({".ts", ".tsx", ".js", ".jsx", ".css", ".html"})
IGNORED_WEB_DIRECTORIES = frozenset(
    {
        "node_modules",
        "dist",
        "coverage",
        ".vite",
        "__tests__",
        "test",
        "tests",
        # Playwright may preserve copied production CSS inside traces after a
        # run.  Those generated artifacts are evidence, not editable source,
        # and scanning them would report the token layer a second time.
        "test-results",
        "playwright-report",
        "blob-report",
    }
)

HEX_COLOR_PATTERN = re.compile(
    r"(?<![0-9A-Za-z_-])#(?:[0-9A-Fa-f]{8}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{4}|[0-9A-Fa-f]{3})(?![0-9A-Fa-f])"
)
FUNCTION_COLOR_PATTERN = re.compile(
    r"\b(?:rgb|rgba|hsl|hsla|hwb|lab|lch|oklab|oklch|color)\s*\(",
    re.IGNORECASE,
)

# CSS Color's named colours are deliberately checked only where a value is
# syntactically plausible.  Scanning the entire stylesheet would incorrectly
# reject selectors such as `.redacted` or custom-property names such as
# `--white-space`.
CSS_NAMED_COLORS = frozenset(
    """
    aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond
    blue blueviolet brown burlywood cadetblue chartreuse chocolate coral
    cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray
    darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid
    darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey
    darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue
    firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod
    gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki
    lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan
    lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon
    lightseagreen lightskyblue lightslategray lightslategrey lightsteelblue
    lightyellow lime limegreen linen magenta maroon mediumaquamarine mediumblue
    mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen
    mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin
    navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod
    palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum
    powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon
    sandybrown seagreen seashell sienna silver skyblue slateblue slategray
    slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet
    wheat white whitesmoke yellow yellowgreen
    """.split()
)
NAMED_COLOR_PATTERN = re.compile(
    r"\b(?:" + "|".join(sorted(CSS_NAMED_COLORS, key=len, reverse=True)) + r")\b",
    re.IGNORECASE,
)
LAYER_TOKENS_PATTERN = re.compile(r"@layer\s+tokens\s*\{", re.IGNORECASE)
MANIFEST_ASSET_PATTERN = re.compile(
    r"web/design-system/assets/illustrations/[A-Za-z0-9][A-Za-z0-9._-]*\.(?:png|webp|avif)\b"
)
SHA256_PATTERN = re.compile(r"\b[A-Fa-f0-9]{64}\b")
ILLUSTRATION_REFERENCE_PATTERN = re.compile(
    r"assets/illustrations/(?P<name>[A-Za-z0-9][A-Za-z0-9._-]*\.(?:png|webp|avif))\b"
)
EMPTY_STATE_IMPORT_PATTERN = re.compile(
    r"import\s*\{(?P<specifiers>[^}]*)\}\s*from\s*[\"']@copilotmail/design-system[\"']",
    re.DOTALL,
)
EMPTY_STATE_NAMESPACE_IMPORT_PATTERN = re.compile(
    r"import\s*\*\s*as\s+(?P<namespace>[A-Za-z_$][\w$]*)\s*from\s*"
    r"[\"']@copilotmail/design-system[\"']",
)
EMPTY_STATE_SPECIFIER_PATTERN = re.compile(
    r"^\s*(?:type\s+)?EmptyState(?:\s+as\s+(?P<alias>[A-Za-z_$][\w$]*))?\s*$"
)
TS_UNICODE_ESCAPE_PATTERN = re.compile(
    r"\\u(?:\{(?P<braced>[0-9A-Fa-f]{1,6})\}|(?P<plain>[0-9A-Fa-f]{4}))"
)
NATIVE_BROWSER_DIALOG_PATTERN = re.compile(
    r"(?<![A-Za-z0-9_$\.\]])(?:(?:window|globalThis|self)\s*\.\s*)?"
    r"(?:alert|confirm|prompt)\s*\("
)
CUSTOM_PROPERTY_PATTERN = re.compile(
    r"(?P<name>--[A-Za-z0-9_-]+)\s*:\s*(?P<value>[^;{}]+);"
)
REDUCED_MOTION_PATTERN = re.compile(
    r"@media\s*\(\s*prefers-reduced-motion\s*:\s*reduce\s*\)\s*\{",
    re.IGNORECASE,
)
OPAQUE_HEX_PATTERN = re.compile(r"#(?P<hex>[0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})")

ACCESSIBILITY_COLOR_PAIRS = (
    ("--cm-info", "--cm-info-soft", 4.5),
    ("--cm-control-border", "--cm-surface", 3.0),
    ("--cm-agent", "--cm-surface", 3.0),
)

# D1 freezes the vocabulary of the shared visual foundation.  The values may
# evolve after an explicit design review, but a feature must never need to
# invent its own font stack, spacing ladder, motion timing, or stacking scale.
FOUNDATION_TOKENS = frozenset(
    {
        "--cm-font-sans",
        "--cm-font-mono",
        "--cm-space-1",
        "--cm-space-2",
        "--cm-space-3",
        "--cm-space-4",
        "--cm-space-5",
        "--cm-space-6",
        "--cm-space-8",
        "--cm-space-10",
        "--cm-space-12",
        "--cm-text-micro",
        "--cm-text-meta",
        "--cm-text-body",
        "--cm-text-reading",
        "--cm-text-section",
        "--cm-text-page",
        "--cm-leading-tight",
        "--cm-leading-normal",
        "--cm-leading-relaxed",
        "--cm-weight-regular",
        "--cm-weight-medium",
        "--cm-weight-semibold",
        "--cm-radius-dialog",
        "--cm-touch-target",
        "--cm-duration-fast",
        "--cm-duration-standard",
        "--cm-duration-slow",
        "--cm-ease-standard",
        "--cm-motion-interactive",
        "--cm-motion-disclosure",
        "--cm-motion-dialog",
        "--cm-z-toast",
        "--cm-z-floating",
        "--cm-z-mobile-picker",
        "--cm-z-inline-notice",
        "--cm-z-command",
        "--cm-z-dialog",
        "--cm-z-session-lock",
    }
)
LOCAL_FONT_FALLBACKS = (
    "Inter",
    '"PingFang SC"',
    '"Microsoft YaHei"',
    '"Noto Sans CJK SC"',
)


class DesignGovernanceError(ValueError):
    """A source-controlled design contract has been violated."""


@dataclass(frozen=True)
class ManifestRecord:
    line_number: int
    purpose: str
    alt: str
    source_sha256: str
    delivery_sha256: str


@dataclass(frozen=True)
class StringSpan:
    start: int
    value: str


def relative(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def is_ignored_web_path(path: Path) -> bool:
    relative_parts = path.relative_to(WEB).parts
    if any(part.lower() in IGNORED_WEB_DIRECTORIES for part in relative_parts[:-1]):
        return True
    lowered_name = path.name.lower()
    return ".test." in lowered_name or ".spec." in lowered_name


def source_files(extensions: frozenset[str]) -> list[Path]:
    return sorted(
        path
        for path in WEB.rglob("*")
        if path.is_file()
        and path.suffix.lower() in extensions
        and not is_ignored_web_path(path)
    )


def mask_css_non_code(text: str) -> str:
    """Blank CSS comments and strings while preserving offsets and line numbers."""

    masked = list(text)
    index = 0
    length = len(text)
    while index < length:
        if text.startswith("/*", index):
            end = text.find("*/", index + 2)
            end = length if end < 0 else end + 2
            for position in range(index, end):
                if masked[position] != "\n":
                    masked[position] = " "
            index = end
            continue
        if text[index] in {"'", '"'}:
            quote = text[index]
            masked[index] = " "
            index += 1
            while index < length:
                current = text[index]
                if current != "\n":
                    masked[index] = " "
                if current == "\\":
                    index += 1
                    if index < length:
                        if text[index] != "\n":
                            masked[index] = " "
                        index += 1
                    continue
                index += 1
                if current == quote:
                    break
            continue
        index += 1
    return "".join(masked)


def matching_brace(masked: str, opening: int) -> int | None:
    depth = 0
    for index in range(opening, len(masked)):
        if masked[index] == "{":
            depth += 1
        elif masked[index] == "}":
            depth -= 1
            if depth == 0:
                return index
    return None


def token_layer_ranges(masked: str) -> tuple[list[tuple[int, int]], list[int]]:
    ranges: list[tuple[int, int]] = []
    unclosed: list[int] = []
    for match in LAYER_TOKENS_PATTERN.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        closing = matching_brace(masked, opening)
        if closing is None:
            unclosed.append(match.start())
        else:
            ranges.append((opening, closing))
    return ranges, unclosed


def offset_in_ranges(offset: int, ranges: list[tuple[int, int]]) -> bool:
    return any(start <= offset <= end for start, end in ranges)


def plausible_css_value(masked: str, offset: int) -> bool:
    """Return whether ``offset`` appears after a declaration colon.

    This tiny parser intentionally does not attempt to validate all CSS.  It
    only prevents named-colour matching from treating selectors/property names
    as values; hexadecimal and functional colours are unambiguous everywhere.
    """

    boundary = max(
        masked.rfind("{", 0, offset),
        masked.rfind("}", 0, offset),
        masked.rfind(";", 0, offset),
    )
    return ":" in masked[boundary + 1 : offset]


def raw_color_occurrences(masked: str) -> list[tuple[int, str]]:
    occurrences: dict[int, str] = {}
    for pattern in (HEX_COLOR_PATTERN, FUNCTION_COLOR_PATTERN):
        for match in pattern.finditer(masked):
            occurrences[match.start()] = match.group(0)
    for match in NAMED_COLOR_PATTERN.finditer(masked):
        if plausible_css_value(masked, match.start()):
            occurrences[match.start()] = match.group(0)
    return sorted(occurrences.items())


def token_declarations(text: str) -> tuple[dict[str, str], list[str]]:
    """Return shared token values and fail closed on duplicate declarations."""

    masked = mask_css_non_code(text)
    ranges, unclosed = token_layer_ranges(masked)
    errors = [
        f"token layer beginning on line {line_number(text, offset)} is not closed"
        for offset in unclosed
    ]
    declarations: dict[str, str] = {}
    for opening, closing in ranges:
        block = masked[opening + 1 : closing]
        for match in CUSTOM_PROPERTY_PATTERN.finditer(block):
            name = match["name"]
            value = match["value"].strip()
            if name in declarations:
                errors.append(f"token {name} is declared more than once")
            else:
                declarations[name] = value
    return declarations, errors


def opaque_hex_rgb(value: str) -> tuple[float, float, float] | None:
    match = OPAQUE_HEX_PATTERN.fullmatch(value.strip())
    if match is None:
        return None
    digits = match["hex"]
    if len(digits) == 3:
        digits = "".join(character * 2 for character in digits)
    return (
        int(digits[0:2], 16) / 255,
        int(digits[2:4], 16) / 255,
        int(digits[4:6], 16) / 255,
    )


def relative_luminance(rgb: tuple[float, float, float]) -> float:
    def linear(channel: float) -> float:
        if channel <= 0.04045:
            return channel / 12.92
        return ((channel + 0.055) / 1.055) ** 2.4

    red, green, blue = (linear(channel) for channel in rgb)
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def contrast_ratio(
    foreground: tuple[float, float, float], background: tuple[float, float, float]
) -> float:
    foreground_luminance = relative_luminance(foreground)
    background_luminance = relative_luminance(background)
    lighter = max(foreground_luminance, background_luminance)
    darker = min(foreground_luminance, background_luminance)
    return (lighter + 0.05) / (darker + 0.05)


def reduced_motion_blocks(text: str) -> list[str]:
    masked = mask_css_non_code(text)
    blocks: list[str] = []
    for match in REDUCED_MOTION_PATTERN.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        closing = matching_brace(masked, opening)
        if closing is not None:
            blocks.append(masked[opening + 1 : closing])
    return blocks


def accessibility_contract_errors(text: str, source_name: str) -> list[str]:
    """Check non-rendering accessibility invariants owned by the token layer."""

    errors: list[str] = []
    declarations, declaration_errors = token_declarations(text)
    errors.extend(f"{source_name}: {error}" for error in declaration_errors)

    for token in sorted(FOUNDATION_TOKENS):
        if token not in declarations:
            errors.append(
                f"{source_name}: missing required V2 foundation token {token}"
            )

    for fallback in LOCAL_FONT_FALLBACKS:
        if fallback not in text:
            errors.append(
                f"{source_name}: --cm-font-sans must retain the local fallback {fallback}"
            )
    if "font-family: var(--cm-font-sans)" not in text:
        errors.append(
            f"{source_name}: the shared root must consume --cm-font-sans"
        )
    if "font-family: var(--cm-font-mono)" not in text:
        errors.append(
            f"{source_name}: code-like content must consume --cm-font-mono"
        )

    parsed_colors: dict[str, tuple[float, float, float]] = {}
    required_colors = {
        token
        for foreground, background, _ in ACCESSIBILITY_COLOR_PAIRS
        for token in (foreground, background)
    }
    for token in sorted(required_colors):
        value = declarations.get(token)
        if value is None:
            errors.append(f"{source_name}: missing required accessibility token {token}")
            continue
        parsed = opaque_hex_rgb(value)
        if parsed is None:
            errors.append(
                f"{source_name}: {token} must be an opaque hexadecimal colour for deterministic contrast checks"
            )
            continue
        parsed_colors[token] = parsed

    for foreground, background, minimum in ACCESSIBILITY_COLOR_PAIRS:
        if foreground not in parsed_colors or background not in parsed_colors:
            continue
        actual = contrast_ratio(parsed_colors[foreground], parsed_colors[background])
        if actual + 1e-9 < minimum:
            errors.append(
                f"{source_name}: {foreground} against {background} has contrast {actual:.2f}:1; "
                f"minimum is {minimum:.1f}:1"
            )

    focus_ring = declarations.get("--cm-focus-ring")
    if focus_ring is None:
        errors.append(f"{source_name}: missing required accessibility token --cm-focus-ring")
    else:
        layers = [layer.strip() for layer in focus_ring.split(",") if layer.strip()]
        surface_layer_count = sum("var(--cm-surface)" in layer for layer in layers)
        agent_layer_count = sum("var(--cm-agent)" in layer for layer in layers)
        if (
            len(layers) != 2
            or surface_layer_count != 1
            or agent_layer_count != 1
            or any(
                "var(--cm-surface)" in layer and "var(--cm-agent)" in layer
                for layer in layers
            )
        ):
            errors.append(
                f"{source_name}: --cm-focus-ring must be exactly two distinct rings using "
                "var(--cm-surface) and var(--cm-agent)"
            )
        if FUNCTION_COLOR_PATTERN.search(focus_ring):
            errors.append(
                f"{source_name}: --cm-focus-ring must use opaque semantic tokens, not rgb/alpha colour functions"
            )

    motion_blocks = reduced_motion_blocks(text)
    iteration_pattern = re.compile(
        r"animation-iteration-count\s*:\s*1\s*!important\s*;", re.IGNORECASE
    )
    spinner_pattern = re.compile(r"\.spinner\b[^{}]*\{(?P<body>[^{}]*)\}", re.IGNORECASE)
    spinner_animation_pattern = re.compile(
        r"(?:^|;)\s*animation\s*:\s*none\s*!important\s*;", re.IGNORECASE
    )
    compliant_motion_block = False
    for block in motion_blocks:
        has_iteration_limit = iteration_pattern.search(block) is not None
        has_static_spinner = any(
            spinner_animation_pattern.search(match["body"]) is not None
            for match in spinner_pattern.finditer(block)
        )
        if has_iteration_limit and has_static_spinner:
            compliant_motion_block = True
            break
    if not compliant_motion_block:
        errors.append(
            f"{source_name}: prefers-reduced-motion must set animation-iteration-count: 1 !important "
            "and .spinner animation: none !important in the same media block"
        )
    return errors


def check_accessibility_contracts(errors: list[str]) -> None:
    if not TOKEN_STYLESHEET.is_file():
        return
    try:
        text = TOKEN_STYLESHEET.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        errors.append(f"cannot read {relative(TOKEN_STYLESHEET)} as UTF-8: {error}")
        return
    errors.extend(accessibility_contract_errors(text, relative(TOKEN_STYLESHEET)))


def check_css_token_ownership(errors: list[str]) -> int:
    if not TOKEN_STYLESHEET.is_file():
        errors.append(f"missing shared token stylesheet: {relative(TOKEN_STYLESHEET)}")
        return 0

    checked = 0
    for stylesheet in source_files(frozenset({".css"})):
        checked += 1
        try:
            text = stylesheet.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"cannot read {relative(stylesheet)} as UTF-8: {error}")
            continue
        masked = mask_css_non_code(text)
        ranges, unclosed = token_layer_ranges(masked)
        for offset in unclosed:
            errors.append(
                f"{relative(stylesheet)}:{line_number(text, offset)} has an unclosed @layer tokens block"
            )
        if stylesheet != TOKEN_STYLESHEET and ranges:
            for start, _ in ranges:
                errors.append(
                    f"{relative(stylesheet)}:{line_number(text, start)} defines @layer tokens; "
                    "tokens belong only to web/design-system/src/styles.css"
                )
        if stylesheet == TOKEN_STYLESHEET and not ranges:
            errors.append(
                f"{relative(stylesheet)} must define the shared @layer tokens block"
            )

        allowed_ranges = ranges if stylesheet == TOKEN_STYLESHEET else []
        for offset, literal in raw_color_occurrences(masked):
            if not offset_in_ranges(offset, allowed_ranges):
                errors.append(
                    f"{relative(stylesheet)}:{line_number(text, offset)} uses raw colour {literal!r}; "
                    "define it in the shared token layer and consume a semantic token here"
                )
    return checked


def parse_manifest_records(text: str) -> tuple[dict[str, ManifestRecord], list[str]]:
    records: dict[str, ManifestRecord] = {}
    errors: list[str] = []
    for number, raw_line in enumerate(text.splitlines(), start=1):
        line = raw_line.strip()
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        asset_paths = MANIFEST_ASSET_PATTERN.findall(raw_line)
        if not asset_paths:
            continue
        if len(cells) < 5:
            errors.append(f"{relative(IMAGE_MANIFEST)}:{number} has an incomplete asset row")
            continue
        purpose, alt, source_hash_cell, delivery_hash_cell = cells[1], cells[2], cells[3], cells[4]
        source_digest = SHA256_PATTERN.search(source_hash_cell)
        delivery_digest = SHA256_PATTERN.search(delivery_hash_cell)
        source_paths = [path for path in asset_paths if Path(path).suffix.lower() == ".png"]
        delivery_paths = [
            path for path in asset_paths
            if Path(path).suffix.lower() in DELIVERY_IMAGE_SUFFIXES
        ]
        if not purpose or purpose == "—":
            errors.append(f"{relative(IMAGE_MANIFEST)}:{number} is missing a concrete screen/state mapping")
        if not alt or alt == "—":
            errors.append(f"{relative(IMAGE_MANIFEST)}:{number} is missing alt text")
        if len(source_paths) != 1 or len(delivery_paths) != 1:
            errors.append(
                f"{relative(IMAGE_MANIFEST)}:{number} must bind exactly one PNG audit source "
                "to exactly one WebP/AVIF delivery asset"
            )
        if source_digest is None:
            errors.append(f"{relative(IMAGE_MANIFEST)}:{number} is missing a source SHA-256 record")
        if delivery_digest is None:
            errors.append(f"{relative(IMAGE_MANIFEST)}:{number} is missing a delivery SHA-256 record")
        if source_digest is None or delivery_digest is None:
            continue
        record = ManifestRecord(
            number,
            purpose,
            alt,
            source_digest.group(0).upper(),
            delivery_digest.group(0).upper(),
        )
        for asset_path in asset_paths:
            if asset_path in records:
                errors.append(
                    f"{relative(IMAGE_MANIFEST)}:{number} records {asset_path} more than once"
                )
            else:
                records[asset_path] = record
    return records, errors


def production_source_files() -> list[Path]:
    return source_files(SOURCE_EXTENSIONS)


def check_image_manifest(errors: list[str]) -> tuple[int, int]:
    if not IMAGE_MANIFEST.is_file():
        errors.append(f"missing Image 2 asset manifest: {relative(IMAGE_MANIFEST)}")
        return 0, 0
    if not ILLUSTRATION_DIRECTORY.is_dir():
        errors.append(
            f"missing governed illustration directory: {relative(ILLUSTRATION_DIRECTORY)}"
        )
        return 0, 0
    try:
        manifest_text = IMAGE_MANIFEST.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        errors.append(f"cannot read {relative(IMAGE_MANIFEST)} as UTF-8: {error}")
        return 0, 0

    records, manifest_errors = parse_manifest_records(manifest_text)
    errors.extend(manifest_errors)
    actual_assets = sorted(
        path
        for path in ILLUSTRATION_DIRECTORY.rglob("*")
        if path.is_file() and path.suffix.lower() in IMAGE_SUFFIXES
    )
    actual_paths = {relative(path) for path in actual_assets}
    if not actual_assets:
        errors.append(f"{relative(ILLUSTRATION_DIRECTORY)} contains no governed raster assets")
    for asset in actual_assets:
        asset_path = relative(asset)
        record = records.get(asset_path)
        if record is None:
            errors.append(
                f"{asset_path} is not registered in {relative(IMAGE_MANIFEST)}"
            )
            continue
        actual_digest = hashlib.sha256(asset.read_bytes()).hexdigest().upper()
        expected_digest = (
            record.source_sha256
            if asset.suffix.lower() == ".png"
            else record.delivery_sha256
        )
        if actual_digest != expected_digest:
            errors.append(
                f"{asset_path} SHA-256 differs from manifest line {record.line_number}: "
                f"expected {expected_digest}, got {actual_digest}"
            )
    for registered_path, record in records.items():
        if registered_path not in actual_paths:
            errors.append(
                f"{relative(IMAGE_MANIFEST)}:{record.line_number} registers missing asset {registered_path}"
            )

    references = 0
    for source in production_source_files():
        try:
            text = source.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"cannot read {relative(source)} as UTF-8: {error}")
            continue
        for match in ILLUSTRATION_REFERENCE_PATTERN.finditer(text):
            references += 1
            filename = match["name"]
            asset_path = (
                "web/design-system/assets/illustrations/" + filename
            )
            if asset_path not in records:
                errors.append(
                    f"{relative(source)}:{line_number(text, match.start())} references "
                    f"unregistered Image 2 asset {asset_path}"
                )
            if asset_path not in actual_paths:
                errors.append(
                    f"{relative(source)}:{line_number(text, match.start())} references "
                    f"missing Image 2 asset {asset_path}"
                )
            if Path(filename).suffix.lower() not in DELIVERY_IMAGE_SUFFIXES:
                errors.append(
                    f"{relative(source)}:{line_number(text, match.start())} references "
                    f"PNG audit source {asset_path}; production UI must use a WebP or AVIF derivative"
                )
    return len(actual_assets), references


def local_empty_state_names(text: str) -> set[str]:
    names: set[str] = set()
    for match in EMPTY_STATE_IMPORT_PATTERN.finditer(text):
        for specifier in match["specifiers"].split(","):
            parsed = EMPTY_STATE_SPECIFIER_PATTERN.match(specifier)
            if parsed is not None:
                names.add(parsed["alias"] or "EmptyState")
    for match in EMPTY_STATE_NAMESPACE_IMPORT_PATTERN.finditer(text):
        names.add(match["namespace"] + ".EmptyState")
    return names


def jsx_opening_tag(text: str, start: int) -> tuple[int, str] | None:
    """Read one JSX opening tag without confusing `>` inside `{...}` values."""

    index = start
    brace_depth = 0
    quote: str | None = None
    while index < len(text):
        current = text[index]
        if quote is not None:
            if current == "\\":
                index += 2
                continue
            if current == quote:
                quote = None
            index += 1
            continue
        if current in {"'", '"', "`"}:
            quote = current
            index += 1
            continue
        if current == "{":
            brace_depth += 1
        elif current == "}" and brace_depth:
            brace_depth -= 1
        elif current == ">" and brace_depth == 0:
            return index + 1, text[start : index + 1]
        index += 1
    return None


def jsx_attribute_present(opening_tag: str, name: str) -> bool:
    return re.search(
        rf"(?<![A-Za-z0-9_$:-]){re.escape(name)}(?![A-Za-z0-9_$:-])(?=\s*=|\s|/|>)",
        opening_tag,
    ) is not None


def check_empty_state_illustrations(errors: list[str]) -> int:
    checked = 0
    for source in source_files(frozenset({".tsx"})):
        try:
            text = source.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"cannot read {relative(source)} as UTF-8: {error}")
            continue
        names = local_empty_state_names(text)
        for name in names:
            tag_pattern = re.compile(r"<\s*" + re.escape(name) + r"\b")
            for match in tag_pattern.finditer(text):
                checked += 1
                parsed = jsx_opening_tag(text, match.start())
                if parsed is None:
                    errors.append(
                        f"{relative(source)}:{line_number(text, match.start())} has an unterminated "
                        f"<{name}> opening tag"
                    )
                    continue
                _, opening_tag = parsed
                if (
                    jsx_attribute_present(opening_tag, "illustration")
                    and not jsx_attribute_present(opening_tag, "illustrationAlt")
                ):
                    errors.append(
                        f"{relative(source)}:{line_number(text, match.start())} gives <{name}> an "
                        "illustration without explicit illustrationAlt"
                    )
    return checked


def typescript_string_spans(text: str) -> list[StringSpan]:
    """Return TypeScript string/template literal spans, excluding comments."""

    spans: list[StringSpan] = []
    index = 0
    while index < len(text):
        if text.startswith("//", index):
            newline = text.find("\n", index + 2)
            index = len(text) if newline < 0 else newline + 1
            continue
        if text.startswith("/*", index):
            closing = text.find("*/", index + 2)
            index = len(text) if closing < 0 else closing + 2
            continue
        quote = text[index]
        if quote not in {"'", '"', "`"}:
            index += 1
            continue
        content_start = index + 1
        index += 1
        while index < len(text):
            current = text[index]
            if current == "\\":
                index += 2
                continue
            if current == quote:
                spans.append(StringSpan(content_start, text[content_start:index]))
                index += 1
                break
            index += 1
        else:
            # Syntax errors belong to the TypeScript checker.  Do not make this
            # governance parser guess where a malformed literal should end.
            break
    return spans


def is_pseudo_icon_codepoint(codepoint: int) -> bool:
    return (
        0x2300 <= codepoint <= 0x23FF
        or 0x2600 <= codepoint <= 0x27BF
        or 0x2B00 <= codepoint <= 0x2BFF
        or 0x1F000 <= codepoint <= 0x1FAFF
        or codepoint in {0x00A9, 0x00AE, 0x203C, 0x2049, 0x2122, 0x2139, 0x3030, 0x303D, 0x3297, 0x3299, 0xFE0E, 0xFE0F}
    )


def pseudo_icon_occurrences(value: str) -> list[tuple[int, int]]:
    occurrences: list[tuple[int, int]] = []
    for index, character in enumerate(value):
        codepoint = ord(character)
        if is_pseudo_icon_codepoint(codepoint):
            occurrences.append((index, codepoint))

    escapes = list(TS_UNICODE_ESCAPE_PATTERN.finditer(value))
    index = 0
    while index < len(escapes):
        match = escapes[index]
        raw_value = match["braced"] or match["plain"]
        assert raw_value is not None
        codepoint = int(raw_value, 16)
        if 0xD800 <= codepoint <= 0xDBFF and index + 1 < len(escapes):
            following = escapes[index + 1]
            following_raw = following["braced"] or following["plain"]
            assert following_raw is not None
            low_surrogate = int(following_raw, 16)
            if following.start() == match.end() and 0xDC00 <= low_surrogate <= 0xDFFF:
                codepoint = 0x10000 + ((codepoint - 0xD800) << 10) + (low_surrogate - 0xDC00)
                index += 1
        if is_pseudo_icon_codepoint(codepoint):
            occurrences.append((match.start(), codepoint))
        index += 1
    return occurrences


def locale_catalog_files() -> list[Path]:
    catalogs: set[Path] = set(WEB.glob("*/src/i18n.ts"))
    for path in WEB.glob("*/src/locales/**/*"):
        if path.is_file() and path.suffix in {".ts", ".tsx"} and not is_ignored_web_path(path):
            catalogs.add(path)
    return sorted(catalogs)


def check_locale_pseudo_icons(errors: list[str]) -> int:
    catalogs = locale_catalog_files()
    if not catalogs:
        errors.append("no Web locale catalog was found under web/*/src")
        return 0
    for catalog in catalogs:
        try:
            text = catalog.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"cannot read {relative(catalog)} as UTF-8: {error}")
            continue
        for span in typescript_string_spans(text):
            for relative_offset, codepoint in pseudo_icon_occurrences(span.value):
                offset = span.start + relative_offset
                errors.append(
                    f"{relative(catalog)}:{line_number(text, offset)} contains pseudo-icon "
                    f"U+{codepoint:04X} in a locale catalog; use the shared SVG icon registry "
                    "for UI icons"
                )
    return len(catalogs)


def check_native_browser_dialogs(errors: list[str]) -> int:
    """Reject browser-owned dialogs that bypass product focus and copy contracts."""

    occurrences = 0
    for source in source_files(frozenset({".ts", ".tsx", ".js", ".jsx"})):
        try:
            text = source.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            errors.append(f"cannot read {relative(source)} as UTF-8: {error}")
            continue
        for match in NATIVE_BROWSER_DIALOG_PATTERN.finditer(text):
            occurrences += 1
            errors.append(
                f"{relative(source)}:{line_number(text, match.start())} uses a native browser "
                "dialog; use the shared accessible Dialog/ConfirmDialog primitives"
            )
    return occurrences


def self_test() -> None:
    if (
        not is_ignored_web_path(
            WEB / "test-results" / ".playwright-artifacts-0" / "trace" / "styles.css"
        )
        or not is_ignored_web_path(WEB / "playwright-report" / "assets" / "styles.css")
        or is_ignored_web_path(WEB / "mail-app" / "src" / "styles.css")
    ):
        raise DesignGovernanceError("self-test generated-artifact boundary drifted")

    token_css = "@layer tokens { :root { --paper: #fff; --shade: rgb(1 2 3); } }"
    feature_css = ".card { color: #fff; background: rgb(1 2 3); } .redacted { color: var(--cm-ink); }"
    token_mask = mask_css_non_code(token_css)
    feature_mask = mask_css_non_code(feature_css)
    ranges, unclosed = token_layer_ranges(token_mask)
    if unclosed or len(ranges) != 1:
        raise DesignGovernanceError("self-test could not isolate the token layer")
    if any(not offset_in_ranges(offset, ranges) for offset, _ in raw_color_occurrences(token_mask)):
        raise DesignGovernanceError("self-test rejected token-layer colours")
    detected_feature_colours = {literal for _, literal in raw_color_occurrences(feature_mask)}
    if "#fff" not in detected_feature_colours or "rgb(" not in detected_feature_colours:
        raise DesignGovernanceError("self-test missed a feature raw colour")
    if any("redacted" in literal for literal in detected_feature_colours):
        raise DesignGovernanceError("self-test misclassified a selector as a colour")

    accessible_css = """
@layer tokens {
  :root {
    --cm-font-sans: Inter, "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", ui-sans-serif, sans-serif;
    --cm-font-mono: Consolas, monospace;
    --cm-surface: #ffffff;
    --cm-agent: #3f39b8;
    --cm-info: #245fc7;
    --cm-info-soft: #eaf1ff;
    --cm-control-border: #8b8780;
    --cm-space-1: 4px;
    --cm-space-2: 8px;
    --cm-space-3: 12px;
    --cm-space-4: 16px;
    --cm-space-5: 20px;
    --cm-space-6: 24px;
    --cm-space-8: 32px;
    --cm-space-10: 40px;
    --cm-space-12: 48px;
    --cm-text-micro: 0.75rem;
    --cm-text-meta: 0.8125rem;
    --cm-text-body: 0.875rem;
    --cm-text-reading: 1rem;
    --cm-text-section: 1.25rem;
    --cm-text-page: 1.75rem;
    --cm-leading-tight: 1.3;
    --cm-leading-normal: 1.45;
    --cm-leading-relaxed: 1.55;
    --cm-weight-regular: 400;
    --cm-weight-medium: 500;
    --cm-weight-semibold: 650;
    --cm-radius-dialog: 14px;
    --cm-touch-target: 44px;
    --cm-duration-fast: 120ms;
    --cm-duration-standard: 160ms;
    --cm-duration-slow: 200ms;
    --cm-ease-standard: cubic-bezier(0.2, 0, 0, 1);
    --cm-motion-interactive: var(--cm-duration-fast) var(--cm-ease-standard);
    --cm-motion-disclosure: var(--cm-duration-standard) var(--cm-ease-standard);
    --cm-motion-dialog: var(--cm-duration-slow) var(--cm-ease-standard);
    --cm-z-toast: 30;
    --cm-z-floating: 40;
    --cm-z-mobile-picker: 50;
    --cm-z-inline-notice: 60;
    --cm-z-command: 70;
    --cm-z-dialog: 80;
    --cm-z-session-lock: 100;
    --cm-focus-ring: 0 0 0 2px var(--cm-surface), 0 0 0 4px var(--cm-agent);
  }
}
body { font-family: var(--cm-font-sans); }
code { font-family: var(--cm-font-mono); }
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-iteration-count: 1 !important; }
  .spinner { animation: none !important; }
}
"""
    if valid_accessibility_errors := accessibility_contract_errors(
        accessible_css, "self-test.css"
    ):
        raise DesignGovernanceError(
            "self-test rejected the accessible visual contract: "
            + "; ".join(valid_accessibility_errors)
        )

    inaccessible_fixtures = (
        (
            accessible_css.replace("--cm-info: #245fc7", "--cm-info: #2f6fed"),
            "--cm-info against --cm-info-soft has contrast",
        ),
        (
            accessible_css.replace(
                "--cm-control-border: #8b8780", "--cm-control-border: #d3cfc6"
            ),
            "--cm-control-border against --cm-surface has contrast",
        ),
        (
            accessible_css.replace("--cm-agent: #3f39b8", "--cm-agent: #d3cfc6"),
            "--cm-agent against --cm-surface has contrast",
        ),
        (
            accessible_css.replace("--cm-space-12: 48px;", ""),
            "missing required V2 foundation token --cm-space-12",
        ),
        (
            accessible_css.replace(
                "0 0 0 2px var(--cm-surface), 0 0 0 4px var(--cm-agent)",
                "0 0 0 3px rgb(63 57 184 / 24%)",
            ),
            "--cm-focus-ring must use opaque semantic tokens",
        ),
        (
            accessible_css.replace(
                "animation-iteration-count: 1 !important;", "animation-duration: 0.01ms !important;"
            ),
            "prefers-reduced-motion must set animation-iteration-count",
        ),
        (
            accessible_css.replace(
                ".spinner { animation: none !important; }",
                ".spinner { animation-duration: 0.01ms !important; }",
            ),
            "prefers-reduced-motion must set animation-iteration-count",
        ),
    )
    for fixture, expected_message in inaccessible_fixtures:
        fixture_errors = accessibility_contract_errors(fixture, "self-test.css")
        if not any(expected_message in error for error in fixture_errors):
            raise DesignGovernanceError(
                f"self-test did not reject an inaccessible fixture via {expected_message!r}"
            )

    manifest = (
        "| File | Purpose | Alt | Source SHA-256 | Delivery SHA-256 |\n"
        "| --- | --- | --- | --- | --- |\n"
        "| `web/design-system/assets/illustrations/sample.png`<br>"
        "`web/design-system/assets/illustrations/sample.webp` | Empty inbox | "
        "Empty tray | " + "A" * 64 + " | " + "B" * 64 + " |\n"
    )
    records, manifest_errors = parse_manifest_records(manifest)
    if manifest_errors or len(records) != 2 or "web/design-system/assets/illustrations/sample.webp" not in records:
        raise DesignGovernanceError("self-test could not parse manifest asset rows")
    missing_delivery_manifest = manifest.replace("B" * 64, "—")
    _, missing_delivery_errors = parse_manifest_records(missing_delivery_manifest)
    if not any("delivery SHA-256" in error for error in missing_delivery_errors):
        raise DesignGovernanceError("self-test accepted an unpinned Image 2 delivery asset")

    jsx = '<EmptyState illustration={asset} illustrationAlt={t("alt")} />'
    missing_alt = '<EmptyState illustration={asset} />'
    parsed = jsx_opening_tag(jsx, 0)
    missing = jsx_opening_tag(missing_alt, 0)
    if (
        parsed is None
        or not jsx_attribute_present(parsed[1], "illustrationAlt")
        or missing is None
        or jsx_attribute_present(missing[1], "illustrationAlt")
    ):
        raise DesignGovernanceError("self-test could not validate EmptyState illustration alt text")
    imported_names = local_empty_state_names(
        'import { EmptyState as State } from "@copilotmail/design-system";\n'
        'import * as DesignSystem from "@copilotmail/design-system";'
    )
    if imported_names != {"State", "DesignSystem.EmptyState"}:
        raise DesignGovernanceError("self-test could not resolve EmptyState import aliases")

    safe_copy = "进度 — {count} · 已完成…"
    icon_copy = "发送 🤖 \\u{1F4E8}"
    if pseudo_icon_occurrences(safe_copy) or len(pseudo_icon_occurrences(icon_copy)) != 2:
        raise DesignGovernanceError("self-test locale pseudo-icon boundary drifted")

    if (
        NATIVE_BROWSER_DIALOG_PATTERN.search("window.confirm('unsafe')") is None
        or NATIVE_BROWSER_DIALOG_PATTERN.search("self.prompt('unsafe')") is None
        or NATIVE_BROWSER_DIALOG_PATTERN.search("confirm('unsafe')") is None
        or NATIVE_BROWSER_DIALOG_PATTERN.search("confirmLocally()") is not None
        or NATIVE_BROWSER_DIALOG_PATTERN.search("dialog.confirm()") is not None
    ):
        raise DesignGovernanceError("self-test native browser dialog boundary drifted")


def validate() -> None:
    errors: list[str] = []
    stylesheet_count = check_css_token_ownership(errors)
    check_accessibility_contracts(errors)
    asset_count, reference_count = check_image_manifest(errors)
    empty_state_count = check_empty_state_illustrations(errors)
    catalog_count = check_locale_pseudo_icons(errors)
    check_native_browser_dialogs(errors)
    if errors:
        for error in errors:
            print(f"design governance error: {error}", file=sys.stderr)
        raise SystemExit(1)
    print(
        "design governance ok: "
        f"{stylesheet_count} feature stylesheets, "
        f"{asset_count} governed raster assets / {reference_count} production references, "
        f"{empty_state_count} EmptyState calls, "
        f"{catalog_count} locale catalogs"
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Check mechanically-verifiable CopilotMail design contracts"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="exercise parser boundaries before checking repository sources",
    )
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        validate()
    except (OSError, UnicodeError, DesignGovernanceError) as error:
        print(f"design governance failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error


if __name__ == "__main__":
    main()
