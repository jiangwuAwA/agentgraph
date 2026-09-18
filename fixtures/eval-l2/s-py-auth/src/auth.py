"""S_py corpus — no eval/exec/compile/getattr-dynamic/ctypes/__import__.

Call chain: validate_email → hash_password → authenticate → login_handler → main
Literal getattr is finite-domain (in S).
"""


class _Registry:
    Service = object


app_registry = _Registry()


def validate_email(email):
    return isinstance(email, str) and "@" in email


def hash_password(password):
    return "h:" + password


def authenticate(email, password):
    if not validate_email(email):
        return {"ok": False, "reason": "email"}
    h = hash_password(password)
    return {"ok": True, "email": email, "hash": h}


def login_handler(email, password):
    return authenticate(email, password)


def get_service():
    return "svc"


def route(svc=None):
    # modeled DI shape — Depends-like default is fine when unused dynamically
    if svc is None:
        svc = get_service()
    return svc


def create_service(registry):
    # finite-domain string-literal getattr — allowed in S
    ctor = getattr(registry, "Service", None)
    return ctor


def main():
    return login_handler("a@b.com", "secret")
