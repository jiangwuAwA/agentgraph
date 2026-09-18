# Public synthetic golden — Python over-flag / clean S slice (M2 / Track M5).
# Literal getattr + literal import_module stay **in S** (must not over-flag).
# Non-literal getattr/eval would leave S — not present here.

import importlib


class Service:
    def load(self) -> int:
        return 1


def call_literal_getattr(svc: Service) -> int:
    # literal attr name — finite domain, stays in S
    return getattr(svc, "load")()


def import_literal() -> object:
    # literal module path — stays in S
    return importlib.import_module("json")
