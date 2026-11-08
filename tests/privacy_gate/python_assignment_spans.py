"""Locate computed regex-group assignments without executing candidate code."""
import ast
import json
import sys

source = sys.stdin.read()
try:
    tree = ast.parse(source)
except (SyntaxError, ValueError, RecursionError):
    print("[]")
    raise SystemExit(0)
spans = []
for node in ast.walk(tree):
    if not isinstance(node, ast.Assign) or len(node.targets) != 1:
        continue
    target, value = node.targets[0], node.value
    if not isinstance(target, ast.Name) or not isinstance(value, ast.Call):
        continue
    method = value.func
    if not (isinstance(method, ast.Attribute) and method.attr == "group"
            and isinstance(method.value, ast.Name) and len(value.args) == 1
            and not value.keywords and isinstance(value.args[0], ast.Constant)
            and type(value.args[0].value) is int and value.args[0].value >= 0):
        continue
    if node.lineno != node.end_lineno or target.lineno != target.end_lineno:
        continue
    spans.append([value.lineno, value.col_offset, value.end_col_offset])
print(json.dumps(spans))
