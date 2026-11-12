# This fixed helper parses input as data. It never imports or executes that input.
import ast
import re
import sys

try:
    raw = sys.stdin.buffer.read(524289)
    if len(raw) > 524288 or b"\r" in raw:
        raise ValueError()
    text = raw.decode("utf-8")
    lines = text.split("\n")
    if len(lines) > 16384:
        raise ValueError()
    tree = ast.parse(text)
    pattern = re.compile(
        r"^([ \t]*)([A-Za-z_][A-Za-z_0-9]*)[ \t]+=[ \t]+"
        r"([A-Za-z_][A-Za-z_0-9]*)\.group\([ \t]*([0-9])[ \t]*\)[ \t]*$"
    )
    accepted = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Assign) or len(node.targets) != 1:
            continue
        if node.lineno != node.end_lineno or not 1 <= node.lineno <= len(lines):
            continue
        match = pattern.fullmatch(lines[node.lineno - 1])
        if not match or not isinstance(node.targets[0], ast.Name):
            continue
        value = node.value
        if not isinstance(value, ast.Call) or not isinstance(value.func, ast.Attribute):
            continue
        if not isinstance(value.func.value, ast.Name) or value.func.attr != "group":
            continue
        if len(value.args) != 1 or value.keywords or not isinstance(value.args[0], ast.Constant):
            continue
        number = value.args[0].value
        if type(number) is not int or not 0 <= number <= 9:
            continue
        if node.targets[0].id != match.group(2) or value.func.value.id != match.group(3):
            continue
        if node.col_offset != len(match.group(1)):
            continue
        accepted.add(node.lineno)
        if len(accepted) > 1024:
            raise ValueError()
    sys.stdout.write("OK\n" + "".join(str(line) + "\n" for line in sorted(accepted)))
except (Exception, RecursionError, MemoryError):
    sys.exit(1)
