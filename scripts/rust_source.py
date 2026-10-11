"""Position-preserving Rust source utilities for lightweight governance checks.

This is not a type checker or macro expander. It masks literals/comments before
finding source-level function bodies; compile/tests remain authoritative.
"""

from dataclasses import dataclass
import re

RAW_STRING = re.compile(r'(?:br|cr|r)(\#*)"')
CHARACTER = re.compile(r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'")


def masked(source: str, keep_strings: bool = False) -> str:
    output = list(source)
    index = 0
    size = len(source)
    while index < size:
        start = index
        is_comment = False
        if source.startswith("//", index):
            index = source.find("\n", index)
            index = size if index < 0 else index
            is_comment = True
        elif source.startswith("/*", index):
            is_comment = True
            depth, index = 1, index + 2
            while index < size and depth:
                if source.startswith("/*", index): depth, index = depth + 1, index + 2
                elif source.startswith("*/", index): depth, index = depth - 1, index + 2
                else: index += 1
            if depth: raise ValueError("unterminated Rust block comment")
        else:
            raw = RAW_STRING.match(source, index)
            char = CHARACTER.match(source, index)
            if raw:
                ending = '"' + raw[1]
                end = source.find(ending, raw.end())
                if end < 0: raise ValueError("unterminated Rust raw string")
                index = end + len(ending)
            elif source[index] == '"':
                index += 1
                while index < size:
                    if source[index] == "\\": index += 2
                    elif source[index] == '"': index += 1; break
                    else: index += 1
                else: raise ValueError("unterminated Rust string")
            elif char:
                index = char.end()
            else:
                index += 1
                continue
        if is_comment or not keep_strings:
            for position in range(start, min(index, size)):
                if source[position] != "\n": output[position] = " "
    return "".join(output)


def block_end(code: str, start: int) -> int:
    depth = 0
    for position in range(start, len(code)):
        if code[position] == "{": depth += 1
        elif code[position] == "}":
            depth -= 1
            if depth == 0: return position + 1
    raise ValueError("unterminated Rust item body")


def production_source(source: str) -> str:
    """Remove test-only modules/functions without dropping later production code."""
    code = masked(source)
    function_items = functions(code)
    ranges = []
    marker = re.compile(r"#\[\s*(?:cfg\s*\(\s*test\s*\)|(?:[a-z_]+\s*::\s*)?test(?:\([^\]]*\))?)\s*\]")
    for match in marker.finditer(code):
        body = code.find("{", match.end())
        semi = code.find(";", match.end())
        next_function = next((item for item in function_items if item.start >= match.end()), None)
        if next_function and (body < 0 or next_function.start < body) and (semi < 0 or next_function.start < semi):
            ranges.append((match.start(), next_function.end))
        elif semi >= 0 and (body < 0 or semi < body):
            ranges.append((match.start(), semi + 1))
        elif body >= 0:
            ranges.append((match.start(), block_end(code, body)))
    result = list(source)
    for start, end in ranges:
        for position in range(start, end):
            if result[position] != "\n": result[position] = " "
    return "".join(result)


@dataclass(frozen=True)
class Function:
    name: str
    start: int
    body_start: int
    end: int


def functions(code: str) -> list[Function]:
    """Find explicit bodies in already masked code, including private functions."""
    result = []
    for match in re.finditer(r"\bfn\s+(?:r#)?([A-Za-z_]\w*)\s*(?:<|\()", code):
        # Parentheses/brackets distinguish a trait terminator from array types.
        parentheses = brackets = 0
        for position in range(match.end() - 1, len(code)):
            char = code[position]
            if char == "(": parentheses += 1
            elif char == ")": parentheses -= 1
            elif char == "[": brackets += 1
            elif char == "]": brackets -= 1
            elif not parentheses and not brackets:
                if char == ";": break
                if char == "{":
                    result.append(Function(match[1], match.start(), position, block_end(code, position)))
                    break
    return result
