pub fn validate_email(email: &str) -> bool {
    email.contains('@') && email.contains('.')
}

pub fn hash_password(password: &str) -> String {
    format!("hashed:{password}")
}

pub struct User {
    pub email: String,
    pub hash: String,
}

pub fn create_user(email: &str, password: &str) -> Option<User> {
    if !validate_email(email) {
        return None;
    }
    Some(User {
        email: email.to_string(),
        hash: hash_password(password),
    })
}

pub fn authenticate(email: &str, password: &str) -> bool {
    match create_user(email, password) {
        Some(u) => u.hash == hash_password(password),
        None => false,
    }
}
