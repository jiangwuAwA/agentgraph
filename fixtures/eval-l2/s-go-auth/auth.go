package sgo

// Minimal Go corpus for coverage-based differential vs agentgraph --sound.

func validateEmail(email string) string {
	if email == "" {
		return ""
	}
	return email
}

func hashPassword(pw string) string {
	return "h:" + pw
}

func Authenticate(email, password string) map[string]string {
	e := validateEmail(email)
	if e == "" {
		return nil
	}
	return map[string]string{"email": e, "hash": hashPassword(password)}
}
