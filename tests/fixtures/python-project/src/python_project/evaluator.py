"""
evaluator.py — Tree-walking interpreter for the python_project expression language.

The :class:`Evaluator` accepts an AST rooted at a
:class:`~python_project.types.Program` node (produced by the
:class:`~python_project.parser.Parser`) and returns the runtime
:data:`~python_project.types.Value` of the program.

Built-in functions
------------------
The global environment is pre-populated with the following functions:

  **Math**: ``abs``, ``round``, ``floor``, ``ceil``, ``max``, ``min``,
  ``pow``, ``sqrt``, ``sign``

  **Type conversion**: ``int``, ``float``, ``bool``, ``str``, ``type``

  **Sequence**: ``len``, ``range``, ``zip``, ``map``, ``filter``, ``list``,
  ``sum``, ``any``, ``all``, ``sorted``, ``reversed``, ``enumerate``

  **String**: ``upper``, ``lower``, ``strip``, ``lstrip``, ``rstrip``,
  ``split``, ``join``, ``contains``, ``startswith``, ``endswith``,
  ``replace``, ``count``, ``index``, ``find``, ``repeat``

  **I/O**: ``print`` (returns ``None``)
"""

from __future__ import annotations

import math
from typing import Callable

from python_project.types import (
    ASTNode,
    BinaryOp,
    BoolLit,
    Call,
    Environment,
    EvalError,
    IfExpr,
    Identifier,
    LetExpr,
    NullLit,
    NumberLit,
    Program,
    StringLit,
    TokenKind,
    UnaryOp,
    Value,
    coerce_to_number,
    coerce_to_string,
    is_truthy,
    type_name,
    value_to_display,
    values_equal,
)

# Type alias for built-in function callables
BuiltinFn = Callable[..., Value]


# ---------------------------------------------------------------------------
# Built-in function implementations
# ---------------------------------------------------------------------------


def _builtin_abs(args: list[Value]) -> Value:
    """Return the absolute value of a single numeric argument."""
    if len(args) != 1:
        raise EvalError(f"abs() takes 1 argument, got {len(args)}")
    n = coerce_to_number(args[0], "abs()")
    return abs(n)


def _builtin_round(args: list[Value]) -> Value:
    """Round a number to ``ndigits`` decimal places (default 0)."""
    if len(args) not in (1, 2):
        raise EvalError(f"round() takes 1 or 2 arguments, got {len(args)}")
    n = coerce_to_number(args[0], "round()")
    if len(args) == 2:
        nd = coerce_to_number(args[1], "round() ndigits")
        if not isinstance(nd, int):
            nd = int(nd)
        return round(n, nd)
    result = round(n)
    return result


def _builtin_floor(args: list[Value]) -> Value:
    """Return the floor of a number (largest integer <= n)."""
    if len(args) != 1:
        raise EvalError(f"floor() takes 1 argument, got {len(args)}")
    n = coerce_to_number(args[0], "floor()")
    return math.floor(n)


def _builtin_ceil(args: list[Value]) -> Value:
    """Return the ceiling of a number (smallest integer >= n)."""
    if len(args) != 1:
        raise EvalError(f"ceil() takes 1 argument, got {len(args)}")
    n = coerce_to_number(args[0], "ceil()")
    return math.ceil(n)


def _builtin_sqrt(args: list[Value]) -> Value:
    """Return the square root of a non-negative number."""
    if len(args) != 1:
        raise EvalError(f"sqrt() takes 1 argument, got {len(args)}")
    n = coerce_to_number(args[0], "sqrt()")
    if isinstance(n, (int, float)) and n < 0:
        raise EvalError("sqrt() argument must be non-negative")
    return math.sqrt(float(n))


def _builtin_pow(args: list[Value]) -> Value:
    """Return base raised to the power of exp."""
    if len(args) != 2:
        raise EvalError(f"pow() takes 2 arguments, got {len(args)}")
    base = coerce_to_number(args[0], "pow() base")
    exp = coerce_to_number(args[1], "pow() exponent")
    result = base**exp
    if isinstance(result, complex):
        raise EvalError("pow() produced a complex number")
    if (
        isinstance(result, float)
        and result == int(result)
        and isinstance(base, int)
        and isinstance(exp, int)
        and exp >= 0
    ):
        return int(result)
    return result


def _builtin_sign(args: list[Value]) -> Value:
    """Return -1, 0, or 1 indicating the sign of a number."""
    if len(args) != 1:
        raise EvalError(f"sign() takes 1 argument, got {len(args)}")
    n = coerce_to_number(args[0], "sign()")
    if n > 0:
        return 1
    if n < 0:
        return -1
    return 0


def _builtin_max(args: list[Value]) -> Value:
    """Return the maximum of two or more numeric arguments."""
    if len(args) == 0:
        raise EvalError("max() requires at least one argument")
    if len(args) == 1 and isinstance(args[0], list):
        lst = args[0]
        if not lst:
            raise EvalError("max() argument is an empty list")
        nums = [coerce_to_number(v, "max()") for v in lst]
        return max(nums)
    nums = [coerce_to_number(v, "max()") for v in args]
    return max(nums)


def _builtin_min(args: list[Value]) -> Value:
    """Return the minimum of two or more numeric arguments."""
    if len(args) == 0:
        raise EvalError("min() requires at least one argument")
    if len(args) == 1 and isinstance(args[0], list):
        lst = args[0]
        if not lst:
            raise EvalError("min() argument is an empty list")
        nums = [coerce_to_number(v, "min()") for v in lst]
        return min(nums)
    nums = [coerce_to_number(v, "min()") for v in args]
    return min(nums)


def _builtin_sum(args: list[Value]) -> Value:
    """Return the sum of a list of numbers."""
    if len(args) != 1:
        raise EvalError(f"sum() takes 1 argument, got {len(args)}")
    lst = args[0]
    if not isinstance(lst, list):
        raise EvalError(f"sum() argument must be a list, got {type_name(lst)}")
    total: int | float = 0
    for item in lst:
        n = coerce_to_number(item, "sum()")
        total = total + n
    return total


def _builtin_compute_score(args: list[Value]) -> Value:
    """Builtin wrapper around :func:`scoring.compute_score`.

    Expects two list arguments: values and weights. Both must be lists of
    numeric values (ints or floats). Returns the weighted mean.
    """
    from python_project.scoring import compute_score

    if len(args) != 2:
        raise EvalError(f"compute_score() takes 2 arguments, got {len(args)}")
    values, weights = args[0], args[1]
    if not isinstance(values, list):
        raise EvalError(
            f"compute_score() values must be a list, got {type_name(values)}"
        )
    if not isinstance(weights, list):
        raise EvalError(
            f"compute_score() weights must be a list, got {type_name(weights)}"
        )
    coerced_values = [coerce_to_number(v, "compute_score()") for v in values]
    coerced_weights = [coerce_to_number(w, "compute_score()") for w in weights]
    return compute_score(coerced_values, coerced_weights)


def _builtin_len(args: list[Value]) -> Value:
    """Return the length of a string or list."""
    if len(args) != 1:
        raise EvalError(f"len() takes 1 argument, got {len(args)}")
    v = args[0]
    if isinstance(v, str):
        return len(v)
    if isinstance(v, list):
        return len(v)
    raise EvalError(f"len() argument must be string or list, got {type_name(v)}")


def _builtin_str(args: list[Value]) -> Value:
    """Convert a value to its string representation."""
    if len(args) != 1:
        raise EvalError(f"str() takes 1 argument, got {len(args)}")
    return coerce_to_string(args[0])


def _builtin_int(args: list[Value]) -> Value:
    """Convert a value to an integer by truncation."""
    if len(args) != 1:
        raise EvalError(f"int() takes 1 argument, got {len(args)}")
    v = args[0]
    if isinstance(v, bool):
        return 1 if v else 0
    if isinstance(v, int):
        return v
    if isinstance(v, float):
        return int(v)
    if isinstance(v, str):
        try:
            return int(v)
        except ValueError:
            try:
                return int(float(v))
            except ValueError:
                raise EvalError(f"Cannot convert string {v!r} to int") from None
    raise EvalError(f"Cannot convert {type_name(v)} to int")


def _builtin_float(args: list[Value]) -> Value:
    """Convert a value to a float."""
    if len(args) != 1:
        raise EvalError(f"float() takes 1 argument, got {len(args)}")
    v = args[0]
    if isinstance(v, bool):
        return float(int(v))
    if isinstance(v, (int, float)):
        return float(v)
    if isinstance(v, str):
        try:
            return float(v)
        except ValueError:
            raise EvalError(f"Cannot convert string {v!r} to float") from None
    raise EvalError(f"Cannot convert {type_name(v)} to float")


def _builtin_bool(args: list[Value]) -> Value:
    """Convert a value to a boolean using truthiness rules."""
    if len(args) != 1:
        raise EvalError(f"bool() takes 1 argument, got {len(args)}")
    return is_truthy(args[0])


def _builtin_type(args: list[Value]) -> Value:
    """Return the type name of a value as a string."""
    if len(args) != 1:
        raise EvalError(f"type() takes 1 argument, got {len(args)}")
    return type_name(args[0])


def _builtin_range(args: list[Value]) -> Value:
    """
    Return a list of integers.

    Signatures:
      range(stop)         → [0, 1, ..., stop-1]
      range(start, stop)  → [start, ..., stop-1]
      range(start, stop, step)
    """
    if len(args) not in (1, 2, 3):
        raise EvalError(f"range() takes 1–3 arguments, got {len(args)}")
    iargs = [int(coerce_to_number(a, "range()")) for a in args]
    if len(iargs) == 1:
        return list(range(iargs[0]))
    if len(iargs) == 2:
        return list(range(iargs[0], iargs[1]))
    if iargs[2] == 0:
        raise EvalError("range() step argument must not be zero")
    return list(range(iargs[0], iargs[1], iargs[2]))


def _builtin_list(args: list[Value]) -> Value:
    """Collect arguments into a list (also handles list literals)."""
    return list(args)


def _builtin_zip(args: list[Value]) -> Value:
    """
    Zip two or more lists element-wise.

    Returns a list of lists, where each inner list contains one element from
    each input list.
    """
    if len(args) < 2:
        raise EvalError(f"zip() requires at least 2 arguments, got {len(args)}")
    for i, a in enumerate(args):
        if not isinstance(a, list):
            raise EvalError(f"zip() argument {i} must be a list, got {type_name(a)}")
    return [list(group) for group in zip(*args)]


def _builtin_enumerate(args: list[Value]) -> Value:
    """
    Return a list of [index, value] pairs from a list.

    Signature: enumerate(lst, start=0)
    """
    if len(args) not in (1, 2):
        raise EvalError(f"enumerate() takes 1 or 2 arguments, got {len(args)}")
    lst = args[0]
    if not isinstance(lst, list):
        raise EvalError(f"enumerate() argument must be a list, got {type_name(lst)}")
    start = 0
    if len(args) == 2:
        start = int(coerce_to_number(args[1], "enumerate() start"))
    return [[start + i, v] for i, v in enumerate(lst)]


def _builtin_sorted(args: list[Value]) -> Value:
    """Return a sorted copy of a list (ascending by default)."""
    if len(args) not in (1, 2):
        raise EvalError(f"sorted() takes 1 or 2 arguments, got {len(args)}")
    lst = args[0]
    if not isinstance(lst, list):
        raise EvalError(f"sorted() argument must be a list, got {type_name(lst)}")
    reverse = False
    if len(args) == 2:
        reverse = bool(is_truthy(args[1]))
    try:
        return sorted(lst, reverse=reverse)  # type: ignore[type-var]
    except TypeError as exc:
        raise EvalError(f"sorted() cannot compare elements: {exc}") from exc


def _builtin_reversed(args: list[Value]) -> Value:
    """Return a reversed copy of a list."""
    if len(args) != 1:
        raise EvalError(f"reversed() takes 1 argument, got {len(args)}")
    lst = args[0]
    if not isinstance(lst, list):
        raise EvalError(f"reversed() argument must be a list, got {type_name(lst)}")
    return list(reversed(lst))


def _builtin_any(args: list[Value]) -> Value:
    """Return True if any element in a list is truthy."""
    if len(args) != 1:
        raise EvalError(f"any() takes 1 argument, got {len(args)}")
    lst = args[0]
    if not isinstance(lst, list):
        raise EvalError(f"any() argument must be a list, got {type_name(lst)}")
    return any(is_truthy(v) for v in lst)


def _builtin_all_fn(args: list[Value]) -> Value:
    """Return True if all elements in a list are truthy."""
    if len(args) != 1:
        raise EvalError(f"all() takes 1 argument, got {len(args)}")
    lst = args[0]
    if not isinstance(lst, list):
        raise EvalError(f"all() argument must be a list, got {type_name(lst)}")
    return all(is_truthy(v) for v in lst)


def _builtin_upper(args: list[Value]) -> Value:
    """Convert a string to upper-case."""
    if len(args) != 1:
        raise EvalError(f"upper() takes 1 argument, got {len(args)}")
    s = args[0]
    if not isinstance(s, str):
        raise EvalError(f"upper() argument must be a string, got {type_name(s)}")
    return s.upper()


def _builtin_lower(args: list[Value]) -> Value:
    """Convert a string to lower-case."""
    if len(args) != 1:
        raise EvalError(f"lower() takes 1 argument, got {len(args)}")
    s = args[0]
    if not isinstance(s, str):
        raise EvalError(f"lower() argument must be a string, got {type_name(s)}")
    return s.lower()


def _builtin_strip(args: list[Value]) -> Value:
    """Strip leading and trailing whitespace (or the given characters) from a string."""
    if len(args) not in (1, 2):
        raise EvalError(f"strip() takes 1 or 2 arguments, got {len(args)}")
    s = args[0]
    if not isinstance(s, str):
        raise EvalError(f"strip() argument must be a string, got {type_name(s)}")
    if len(args) == 2:
        chars = args[1]
        if not isinstance(chars, str):
            raise EvalError("strip() second argument must be a string")
        return s.strip(chars)
    return s.strip()


def _builtin_lstrip(args: list[Value]) -> Value:
    """Strip leading whitespace from a string."""
    if len(args) not in (1, 2):
        raise EvalError(f"lstrip() takes 1 or 2 arguments, got {len(args)}")
    s = args[0]
    if not isinstance(s, str):
        raise EvalError(f"lstrip() argument must be a string, got {type_name(s)}")
    if len(args) == 2:
        chars = args[1]
        if not isinstance(chars, str):
            raise EvalError("lstrip() second argument must be a string")
        return s.lstrip(chars)
    return s.lstrip()


def _builtin_rstrip(args: list[Value]) -> Value:
    """Strip trailing whitespace from a string."""
    if len(args) not in (1, 2):
        raise EvalError(f"rstrip() takes 1 or 2 arguments, got {len(args)}")
    s = args[0]
    if not isinstance(s, str):
        raise EvalError(f"rstrip() argument must be a string, got {type_name(s)}")
    if len(args) == 2:
        chars = args[1]
        if not isinstance(chars, str):
            raise EvalError("rstrip() second argument must be a string")
        return s.rstrip(chars)
    return s.rstrip()


def _builtin_split(args: list[Value]) -> Value:
    """
    Split a string into a list of substrings.

    Signatures:
      split(s)           → split on whitespace
      split(s, sep)      → split on separator string
      split(s, sep, max) → split at most max times
    """
    if len(args) not in (1, 2, 3):
        raise EvalError(f"split() takes 1–3 arguments, got {len(args)}")
    s = args[0]
    if not isinstance(s, str):
        raise EvalError(f"split() first argument must be a string, got {type_name(s)}")
    sep: str | None = None
    if len(args) >= 2:
        sep_val = args[1]
        if not isinstance(sep_val, str):
            raise EvalError("split() separator must be a string")
        sep = sep_val if sep_val else None
    maxsplit = -1
    if len(args) == 3:
        maxsplit = int(coerce_to_number(args[2], "split() maxsplit"))
    return s.split(sep, maxsplit)


def _builtin_join(args: list[Value]) -> Value:
    """
    Join a list of strings with a separator.

    Signature: join(sep, lst)
    """
    if len(args) != 2:
        raise EvalError(f"join() takes 2 arguments, got {len(args)}")
    sep = args[0]
    lst = args[1]
    if not isinstance(sep, str):
        raise EvalError(f"join() separator must be a string, got {type_name(sep)}")
    if not isinstance(lst, list):
        raise EvalError(f"join() second argument must be a list, got {type_name(lst)}")
    parts: list[str] = []
    for i, item in enumerate(lst):
        if not isinstance(item, str):
            raise EvalError(
                f"join() list element {i} must be a string, got {type_name(item)}"
            )
        parts.append(item)
    return sep.join(parts)


def _builtin_contains(args: list[Value]) -> Value:
    """
    Check membership.

    Signatures:
      contains(haystack_str, needle_str) → bool
      contains(lst, value)               → bool
    """
    if len(args) != 2:
        raise EvalError(f"contains() takes 2 arguments, got {len(args)}")
    container = args[0]
    item = args[1]
    if isinstance(container, str):
        if not isinstance(item, str):
            raise EvalError(
                "contains() second argument must be a string when first is a string"
            )
        return item in container
    if isinstance(container, list):
        return any(values_equal(v, item) for v in container)
    raise EvalError(
        f"contains() first argument must be string or list, got {type_name(container)}"
    )


def _builtin_startswith(args: list[Value]) -> Value:
    """Return True if a string starts with the given prefix."""
    if len(args) != 2:
        raise EvalError(f"startswith() takes 2 arguments, got {len(args)}")
    s, prefix = args[0], args[1]
    if not isinstance(s, str):
        raise EvalError(
            f"startswith() first argument must be string, got {type_name(s)}"
        )
    if not isinstance(prefix, str):
        raise EvalError("startswith() prefix must be a string")
    return s.startswith(prefix)


def _builtin_endswith(args: list[Value]) -> Value:
    """Return True if a string ends with the given suffix."""
    if len(args) != 2:
        raise EvalError(f"endswith() takes 2 arguments, got {len(args)}")
    s, suffix = args[0], args[1]
    if not isinstance(s, str):
        raise EvalError(f"endswith() first argument must be string, got {type_name(s)}")
    if not isinstance(suffix, str):
        raise EvalError("endswith() suffix must be a string")
    return s.endswith(suffix)


def _builtin_replace(args: list[Value]) -> Value:
    """Replace occurrences of old with new in a string."""
    if len(args) not in (3, 4):
        raise EvalError(f"replace() takes 3 or 4 arguments, got {len(args)}")
    s, old, new_str = args[0], args[1], args[2]
    if not isinstance(s, str):
        raise EvalError(f"replace() first argument must be string, got {type_name(s)}")
    if not isinstance(old, str):
        raise EvalError("replace() old must be a string")
    if not isinstance(new_str, str):
        raise EvalError("replace() new must be a string")
    if len(args) == 4:
        count = int(coerce_to_number(args[3], "replace() count"))
        return s.replace(old, new_str, count)
    return s.replace(old, new_str)


def _builtin_count(args: list[Value]) -> Value:
    """Count occurrences of a substring in a string, or an item in a list."""
    if len(args) != 2:
        raise EvalError(f"count() takes 2 arguments, got {len(args)}")
    container, item = args[0], args[1]
    if isinstance(container, str):
        if not isinstance(item, str):
            raise EvalError("count() item must be a string when container is a string")
        return container.count(item)
    if isinstance(container, list):
        return sum(1 for v in container if values_equal(v, item))
    raise EvalError(
        f"count() first argument must be string or list, got {type_name(container)}"
    )


def _builtin_find(args: list[Value]) -> Value:
    """Return the index of the first occurrence of needle in haystack, or -1."""
    if len(args) != 2:
        raise EvalError(f"find() takes 2 arguments, got {len(args)}")
    s, needle = args[0], args[1]
    if not isinstance(s, str):
        raise EvalError(f"find() first argument must be string, got {type_name(s)}")
    if not isinstance(needle, str):
        raise EvalError("find() needle must be a string")
    return s.find(needle)


def _builtin_index(args: list[Value]) -> Value:
    """Return the index of the first occurrence of an item in a list."""
    if len(args) != 2:
        raise EvalError(f"index() takes 2 arguments, got {len(args)}")
    container, item = args[0], args[1]
    if isinstance(container, list):
        for i, v in enumerate(container):
            if values_equal(v, item):
                return i
        raise EvalError("index(): item not found in list")
    if isinstance(container, str):
        if not isinstance(item, str):
            raise EvalError("index() item must be a string when container is a string")
        idx = container.find(item)
        if idx == -1:
            raise EvalError(f"index(): substring {item!r} not found")
        return idx
    raise EvalError(
        f"index() first argument must be string or list, got {type_name(container)}"
    )


def _builtin_repeat(args: list[Value]) -> Value:
    """Repeat a string or list n times."""
    if len(args) != 2:
        raise EvalError(f"repeat() takes 2 arguments, got {len(args)}")
    container, count_val = args[0], args[1]
    n = int(coerce_to_number(count_val, "repeat() count"))
    if isinstance(container, str):
        return container * n
    if isinstance(container, list):
        return container * n
    raise EvalError(
        f"repeat() first argument must be string or list, got {type_name(container)}"
    )


def _builtin_print(args: list[Value]) -> Value:
    """Print arguments to stdout separated by spaces, return None."""
    parts = [value_to_display(a) for a in args]
    print(" ".join(parts))
    return None


# ---------------------------------------------------------------------------
# Built-in function registry
# ---------------------------------------------------------------------------


def _make_global_env() -> Environment:
    """
    Create the global environment pre-populated with all built-in functions.

    Each built-in is stored as a Python callable.  The evaluator detects
    callable values in :meth:`Evaluator._eval_call` and dispatches
    accordingly.

    Returns:
        A fully-initialised :class:`~python_project.types.Environment`.
    """
    env = Environment()

    builtins: dict[str, BuiltinFn] = {
        # Math
        "abs": _builtin_abs,
        "round": _builtin_round,
        "floor": _builtin_floor,
        "ceil": _builtin_ceil,
        "sqrt": _builtin_sqrt,
        "pow": _builtin_pow,
        "sign": _builtin_sign,
        "max": _builtin_max,
        "min": _builtin_min,
        "sum": _builtin_sum,
        # Scoring (added per ADR DEC-0017)
        "compute_score": _builtin_compute_score,
        # Type conversion
        "int": _builtin_int,
        "float": _builtin_float,
        "bool": _builtin_bool,
        "str": _builtin_str,
        "type": _builtin_type,
        # Sequence
        "len": _builtin_len,
        "range": _builtin_range,
        "list": _builtin_list,
        "zip": _builtin_zip,
        "enumerate": _builtin_enumerate,
        "sorted": _builtin_sorted,
        "reversed": _builtin_reversed,
        "any": _builtin_any,
        "all": _builtin_all_fn,
        # String
        "upper": _builtin_upper,
        "lower": _builtin_lower,
        "strip": _builtin_strip,
        "lstrip": _builtin_lstrip,
        "rstrip": _builtin_rstrip,
        "split": _builtin_split,
        "join": _builtin_join,
        "contains": _builtin_contains,
        "startswith": _builtin_startswith,
        "endswith": _builtin_endswith,
        "replace": _builtin_replace,
        "count": _builtin_count,
        "find": _builtin_find,
        "index": _builtin_index,
        "repeat": _builtin_repeat,
        # I/O
        "print": _builtin_print,
        # Math constants
        "pi": math.pi,
        "e": math.e,
        "inf": math.inf,
        "nan": math.nan,
    }

    for name, fn_or_val in builtins.items():
        env.define(name, fn_or_val)  # type: ignore[arg-type]

    return env


# ---------------------------------------------------------------------------
# Evaluator class
# ---------------------------------------------------------------------------


class Evaluator:
    """
    Tree-walking interpreter for the python_project expression language.

    The evaluator walks the AST produced by the :class:`~python_project.parser.Parser`
    and returns the runtime :data:`~python_project.types.Value` of the program.

    Each ``_eval_*`` method corresponds to one AST node type.

    Attributes:
        _env: The active :class:`~python_project.types.Environment` (global
              scope unless a child env was passed in).

    Examples::

        from python_project.parser import Parser
        from python_project.lexer import Lexer

        tree = Parser(Lexer("2 + 3 * 4").tokenize()).parse()
        result = Evaluator().evaluate(tree)
        assert result == 14
    """

    def __init__(self, env: Environment | None = None) -> None:
        """
        Initialise the Evaluator.

        Args:
            env: An optional :class:`~python_project.types.Environment` to
                 use as the global scope.  If ``None``, a new global
                 environment pre-populated with all built-in functions is
                 created via :func:`_make_global_env`.
        """
        self._env: Environment = env if env is not None else _make_global_env()

    # ------------------------------------------------------------------
    # Public interface
    # ------------------------------------------------------------------

    def evaluate(self, node: ASTNode) -> Value:
        """
        Evaluate an AST node and return its runtime value.

        This is the main entry point.  It dispatches to the appropriate
        ``_eval_*`` method based on the node type.

        Args:
            node: Any :class:`~python_project.types.ASTNode` — typically the
                  root :class:`~python_project.types.Program` node.

        Returns:
            The runtime :data:`~python_project.types.Value` of the node.

        Raises:
            EvalError: On runtime errors (type mismatch, division by zero,
                       undefined variable, wrong argument count, etc.).

        Examples::

            tree = Parser(Lexer("1 + 2").tokenize()).parse()
            Evaluator().evaluate(tree)  # 3
        """
        return self._dispatch(node)

    def eval_source(self, source: str) -> Value:
        """
        Lex, parse, and evaluate a source string in one call.

        Args:
            source: The expression language source code.

        Returns:
            The runtime value of the last expression in the program.

        Raises:
            LexError:   On a lexical error.
            ParseError: On a parse error.
            EvalError:  On a runtime error.

        Examples::

            Evaluator().eval_source("let x = 5 in x * x")  # 25
        """
        from python_project.lexer import Lexer  # noqa: PLC0415
        from python_project.parser import Parser  # noqa: PLC0415

        tokens = Lexer(source).tokenize()
        tree = Parser(tokens).parse()
        return self.evaluate(tree)

    @property
    def env(self) -> Environment:
        """The active global environment of this evaluator."""
        return self._env

    # ------------------------------------------------------------------
    # Dispatch
    # ------------------------------------------------------------------

    def _dispatch(self, node: ASTNode) -> Value:
        """
        Dispatch evaluation to the correct ``_eval_*`` method.

        Args:
            node: The AST node to evaluate.

        Returns:
            The runtime value.

        Raises:
            EvalError: For unknown node types.
        """
        if isinstance(node, Program):
            return self._eval_program(node)
        if isinstance(node, NumberLit):
            return self._eval_number(node)
        if isinstance(node, StringLit):
            return self._eval_string(node)
        if isinstance(node, BoolLit):
            return self._eval_bool(node)
        if isinstance(node, NullLit):
            return self._eval_null(node)
        if isinstance(node, Identifier):
            return self._eval_identifier(node)
        if isinstance(node, BinaryOp):
            return self._eval_binary(node)
        if isinstance(node, UnaryOp):
            return self._eval_unary(node)
        if isinstance(node, Call):
            return self._eval_call(node)
        if isinstance(node, IfExpr):
            return self._eval_if(node)
        if isinstance(node, LetExpr):
            return self._eval_let(node)
        raise EvalError(
            f"Unknown AST node type: {type(node).__name__}",
            line=node.line,
            col=node.col,
        )

    # ------------------------------------------------------------------
    # Literal node evaluators
    # ------------------------------------------------------------------

    def _eval_program(self, node: Program) -> Value:
        """
        Evaluate a :class:`~python_project.types.Program` node.

        Evaluates each expression in ``body`` in order and returns the value
        of the last one.  An empty program returns ``None``.

        Args:
            node: The program root node.

        Returns:
            The value of the final expression, or ``None`` for an empty program.
        """
        result: Value = None
        for expr in node.body:
            result = self._dispatch(expr)
        return result

    def _eval_number(self, node: NumberLit) -> Value:
        """
        Evaluate a :class:`~python_project.types.NumberLit` node.

        Args:
            node: The numeric literal node.

        Returns:
            The ``int`` or ``float`` value.
        """
        return node.value

    def _eval_string(self, node: StringLit) -> Value:
        """
        Evaluate a :class:`~python_project.types.StringLit` node.

        Args:
            node: The string literal node.

        Returns:
            The Python ``str`` value.
        """
        return node.value

    def _eval_bool(self, node: BoolLit) -> Value:
        """
        Evaluate a :class:`~python_project.types.BoolLit` node.

        Args:
            node: The boolean literal node.

        Returns:
            ``True`` or ``False``.
        """
        return node.value

    def _eval_null(self, node: NullLit) -> Value:
        """
        Evaluate a :class:`~python_project.types.NullLit` node.

        Args:
            node: The null literal node.

        Returns:
            ``None``.
        """
        return None

    # ------------------------------------------------------------------
    # Variable reference
    # ------------------------------------------------------------------

    def _eval_identifier(self, node: Identifier) -> Value:
        """
        Evaluate an :class:`~python_project.types.Identifier` node by
        looking up the variable name in the current environment chain.

        Args:
            node: The identifier node.

        Returns:
            The value bound to the name.

        Raises:
            EvalError: If the name is not defined in any enclosing scope.
        """
        try:
            return self._env.lookup(node.name)
        except EvalError as exc:
            raise EvalError(str(exc), line=node.line, col=node.col) from exc

    # ------------------------------------------------------------------
    # Binary operations
    # ------------------------------------------------------------------

    def _eval_binary(self, node: BinaryOp) -> Value:
        """
        Evaluate a :class:`~python_project.types.BinaryOp` node.

        Short-circuit evaluation is applied for ``and`` and ``or``:
          - ``a and b``: evaluates ``b`` only if ``a`` is truthy.
          - ``a or b``:  evaluates ``b`` only if ``a`` is falsy.

        Args:
            node: The binary operation node.

        Returns:
            The result of the operation.

        Raises:
            EvalError: On type mismatch, division by zero, or unknown operator.
        """
        op_kind = node.op.kind

        # Short-circuit logical operators
        if op_kind == TokenKind.AND:
            left_val = self._dispatch(node.left)
            if not is_truthy(left_val):
                return left_val
            return self._dispatch(node.right)

        if op_kind == TokenKind.OR:
            left_val = self._dispatch(node.left)
            if is_truthy(left_val):
                return left_val
            return self._dispatch(node.right)

        # Evaluate both sides eagerly for all other operators
        left_val = self._dispatch(node.left)
        right_val = self._dispatch(node.right)

        line, col = node.op.line, node.op.col

        if op_kind == TokenKind.PLUS:
            return self._op_add(left_val, right_val, line, col)
        if op_kind == TokenKind.MINUS:
            return self._op_sub(left_val, right_val, line, col)
        if op_kind == TokenKind.STAR:
            return self._op_mul(left_val, right_val, line, col)
        if op_kind == TokenKind.SLASH:
            return self._op_div(left_val, right_val, line, col)
        if op_kind == TokenKind.PERCENT:
            return self._op_mod(left_val, right_val, line, col)
        if op_kind == TokenKind.CARET:
            return self._op_pow(left_val, right_val, line, col)
        if op_kind == TokenKind.EQ:
            return values_equal(left_val, right_val)
        if op_kind == TokenKind.NEQ:
            return not values_equal(left_val, right_val)
        if op_kind == TokenKind.LT:
            return self._op_compare(left_val, right_val, "<", line, col)
        if op_kind == TokenKind.LTE:
            return self._op_compare(left_val, right_val, "<=", line, col)
        if op_kind == TokenKind.GT:
            return self._op_compare(left_val, right_val, ">", line, col)
        if op_kind == TokenKind.GTE:
            return self._op_compare(left_val, right_val, ">=", line, col)

        raise EvalError(
            f"Unknown binary operator {node.op.kind.name}",
            line=line,
            col=col,
        )

    def _op_add(self, left: Value, right: Value, line: int, col: int) -> Value:
        """
        Implement the ``+`` operator.

        Supports:
          - Number + Number → Number
          - String + String → String (concatenation)
          - String + Number → String (concatenation with stringified number)
          - Number + String → String

        Args:
            left:  Left operand.
            right: Right operand.
            line:  Source line for error reporting.
            col:   Source column for error reporting.

        Returns:
            The sum or concatenated value.

        Raises:
            EvalError: On an unsupported type combination.
        """
        if isinstance(left, bool) or isinstance(right, bool):
            # Prevent bool + number being treated as int arithmetic silently
            if isinstance(left, bool) and isinstance(right, bool):
                return int(left) + int(right)
            if isinstance(left, bool):
                left = int(left)
            if isinstance(right, bool):
                right = int(right)
        if isinstance(left, (int, float)) and isinstance(right, (int, float)):
            result = left + right
            if isinstance(left, int) and isinstance(right, int):
                return int(result)
            return float(result)
        if isinstance(left, str) and isinstance(right, str):
            return left + right
        if isinstance(left, str):
            return left + coerce_to_string(right)
        if isinstance(right, str):
            return coerce_to_string(left) + right
        if isinstance(left, list) and isinstance(right, list):
            return left + right
        raise EvalError(
            f"Cannot add {type_name(left)} and {type_name(right)}",
            line=line,
            col=col,
        )

    def _op_sub(self, left: Value, right: Value, line: int, col: int) -> Value:
        """
        Implement the ``-`` operator (subtraction).

        Args:
            left:  Left numeric operand.
            right: Right numeric operand.
            line:  Source line.
            col:   Source column.

        Returns:
            The difference.

        Raises:
            EvalError: If either operand is not a number.
        """
        l = coerce_to_number(left, f"'-' operator left operand [{line}:{col}]")
        r = coerce_to_number(right, f"'-' operator right operand [{line}:{col}]")
        result = l - r
        if isinstance(l, int) and isinstance(r, int):
            return int(result)
        return float(result)

    def _op_mul(self, left: Value, right: Value, line: int, col: int) -> Value:
        """
        Implement the ``*`` operator (multiplication).

        Also supports string/list repetition:
          - string * int → string repeated
          - int * string → string repeated
          - list * int   → list repeated

        Args:
            left:  Left operand.
            right: Right operand.
            line:  Source line.
            col:   Source column.

        Returns:
            The product or repeated value.

        Raises:
            EvalError: On invalid type combination.
        """
        if isinstance(left, str) and isinstance(right, (int, float)):
            return left * int(right)
        if isinstance(left, (int, float)) and isinstance(right, str):
            return right * int(left)
        if isinstance(left, list) and isinstance(right, (int, float)):
            return left * int(right)
        l = coerce_to_number(left, f"'*' operator left [{line}:{col}]")
        r = coerce_to_number(right, f"'*' operator right [{line}:{col}]")
        result = l * r
        if isinstance(l, int) and isinstance(r, int):
            return int(result)
        return float(result)

    def _op_div(self, left: Value, right: Value, line: int, col: int) -> Value:
        """
        Implement the ``/`` operator (true division, always returns float).

        Args:
            left:  Left numeric operand.
            right: Right numeric operand.
            line:  Source line.
            col:   Source column.

        Returns:
            The quotient as a ``float``.

        Raises:
            EvalError: On division by zero or non-numeric operands.
        """
        l = coerce_to_number(left, f"'/' operator left [{line}:{col}]")
        r = coerce_to_number(right, f"'/' operator right [{line}:{col}]")
        if r == 0:
            raise EvalError("Division by zero", line=line, col=col)
        return l / r

    def _op_mod(self, left: Value, right: Value, line: int, col: int) -> Value:
        """
        Implement the ``%`` operator (modulo).

        Args:
            left:  Left numeric operand.
            right: Right numeric operand.
            line:  Source line.
            col:   Source column.

        Returns:
            The remainder.

        Raises:
            EvalError: On modulo by zero or non-numeric operands.
        """
        l = coerce_to_number(left, f"'%%' operator left [{line}:{col}]")
        r = coerce_to_number(right, f"'%%' operator right [{line}:{col}]")
        if r == 0:
            raise EvalError("Modulo by zero", line=line, col=col)
        result = l % r
        if isinstance(l, int) and isinstance(r, int):
            return int(result)
        return float(result)

    def _op_pow(self, left: Value, right: Value, line: int, col: int) -> Value:
        """
        Implement the ``^`` operator (exponentiation).

        Args:
            left:  Base numeric operand.
            right: Exponent numeric operand.
            line:  Source line.
            col:   Source column.

        Returns:
            The result of ``left ** right``.

        Raises:
            EvalError: On non-numeric operands or complex result.
        """
        l = coerce_to_number(left, f"'^' operator base [{line}:{col}]")
        r = coerce_to_number(right, f"'^' operator exponent [{line}:{col}]")
        try:
            result = l**r
        except ZeroDivisionError as exc:
            raise EvalError(
                "Zero raised to negative power", line=line, col=col
            ) from exc
        if isinstance(result, complex):
            raise EvalError(
                "Exponentiation produced a complex number", line=line, col=col
            )
        if isinstance(l, int) and isinstance(r, int) and r >= 0:
            return int(result)
        return float(result)

    def _op_compare(
        self, left: Value, right: Value, op: str, line: int, col: int
    ) -> Value:
        """
        Implement a numeric or string comparison operator.

        Supports comparing two numbers or two strings (lexicographic).

        Args:
            left:  Left operand.
            right: Right operand.
            op:    One of ``"<"``, ``"<="``, ``">"``, ``">="``.
            line:  Source line.
            col:   Source column.

        Returns:
            ``True`` or ``False``.

        Raises:
            EvalError: If the operands cannot be compared.
        """
        if isinstance(left, (int, float)) and isinstance(right, (int, float)):
            if not isinstance(left, bool) and not isinstance(right, bool):
                if op == "<":
                    return left < right
                if op == "<=":
                    return left <= right
                if op == ">":
                    return left > right
                if op == ">=":
                    return left >= right
        if isinstance(left, str) and isinstance(right, str):
            if op == "<":
                return left < right
            if op == "<=":
                return left <= right
            if op == ">":
                return left > right
            if op == ">=":
                return left >= right
        raise EvalError(
            f"Cannot compare {type_name(left)} {op} {type_name(right)}",
            line=line,
            col=col,
        )

    # ------------------------------------------------------------------
    # Unary operations
    # ------------------------------------------------------------------

    def _eval_unary(self, node: UnaryOp) -> Value:
        """
        Evaluate a :class:`~python_project.types.UnaryOp` node.

        Supported operators:
          - ``-`` (numeric negation)
          - ``not`` (logical negation)

        Args:
            node: The unary operation node.

        Returns:
            The result of applying the unary operator to its operand.

        Raises:
            EvalError: On type mismatch or unknown operator.
        """
        operand_val = self._dispatch(node.operand)
        line, col = node.op.line, node.op.col

        if node.op.kind == TokenKind.MINUS:
            n = coerce_to_number(operand_val, f"unary '-' [{line}:{col}]")
            if isinstance(n, int):
                return -n
            return -float(n)

        if node.op.kind == TokenKind.NOT:
            return not is_truthy(operand_val)

        raise EvalError(
            f"Unknown unary operator {node.op.kind.name}",
            line=line,
            col=col,
        )

    # ------------------------------------------------------------------
    # Function call
    # ------------------------------------------------------------------

    def _eval_call(self, node: Call) -> Value:
        """
        Evaluate a :class:`~python_project.types.Call` node.

        Looks up the callee, evaluates all argument expressions, and invokes
        the function.  Built-in functions are Python callables stored in the
        environment.

        Args:
            node: The call node.

        Returns:
            The function's return value.

        Raises:
            EvalError: If the callee is not callable, or the built-in raises
                       an error.
        """
        callee_val = self._dispatch(node.callee)
        arg_vals: list[Value] = [self._dispatch(arg) for arg in node.args]

        if callable(callee_val):
            try:
                return callee_val(arg_vals)
            except EvalError:
                raise
            except Exception as exc:
                raise EvalError(
                    f"Error in built-in function: {exc}",
                    line=node.line,
                    col=node.col,
                ) from exc

        raise EvalError(
            f"'{getattr(node.callee, 'name', '?')}' is not a function "
            f"(got {type_name(callee_val)})",
            line=node.line,
            col=node.col,
        )

    # ------------------------------------------------------------------
    # Special forms
    # ------------------------------------------------------------------

    def _eval_if(self, node: IfExpr) -> Value:
        """
        Evaluate an :class:`~python_project.types.IfExpr` node.

        Only one branch (``then_branch`` or ``else_branch``) is evaluated,
        depending on the truthiness of the condition.

        Args:
            node: The if expression node.

        Returns:
            The value of the taken branch, or ``None`` if the else branch is
            absent and the condition is falsy.
        """
        condition_val = self._dispatch(node.condition)
        if is_truthy(condition_val):
            return self._dispatch(node.then_branch)
        if node.else_branch is not None:
            return self._dispatch(node.else_branch)
        return None

    def _eval_let(self, node: LetExpr) -> Value:
        """
        Evaluate a :class:`~python_project.types.LetExpr` node.

        Creates a child environment, binds the name to the evaluated binding
        expression, then evaluates the body in that child environment.

        Args:
            node: The let expression node.

        Returns:
            The value of the body expression.
        """
        binding_val = self._dispatch(node.binding)
        child_env = self._env.child()
        child_env.define(node.name, binding_val)

        # Temporarily swap the active environment
        saved_env = self._env
        self._env = child_env
        try:
            result = self._dispatch(node.body)
        finally:
            self._env = saved_env

        return result

    # ------------------------------------------------------------------
    # Utility helpers
    # ------------------------------------------------------------------

    def define(self, name: str, value: Value) -> None:
        """
        Bind ``name`` to ``value`` in the global environment.

        Args:
            name:  Variable name.
            value: Runtime value.
        """
        self._env.define(name, value)

    def lookup(self, name: str) -> Value:
        """
        Look up a name in the current environment chain.

        Args:
            name: Variable name.

        Returns:
            The bound value.

        Raises:
            EvalError: If the name is not defined.
        """
        return self._env.lookup(name)

    def __repr__(self) -> str:
        """Return a concise representation of this evaluator."""
        return f"Evaluator(env_depth={self._env.depth()})"
