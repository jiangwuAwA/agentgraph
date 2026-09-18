"""S_py evil corpus — eval leaves S (subset_ok must be false)."""


def dangerous(code):
    return eval(code)
