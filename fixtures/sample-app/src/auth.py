def validate_email(email: str) -> bool:
    return "@" in email and "." in email


def create_user(email: str, password: str) -> dict:
    if not validate_email(email):
        raise ValueError("invalid email")
    return {"email": email, "password": password}


def login(email: str, password: str) -> bool:
    user = create_user(email, password)
    return user["email"] == email
