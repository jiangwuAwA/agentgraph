package auth

import "strings"

// ValidateEmail checks for a basic email shape.
func ValidateEmail(email string) bool {
	return strings.Contains(email, "@") && strings.Contains(email, ".")
}

// User is a minimal account record.
type User struct {
	Email string
	Hash  string
}

// HashPassword is a stub hash.
func HashPassword(password string) string {
	return "hashed:" + password
}

// CreateUser validates and builds a user.
func CreateUser(email, password string) (*User, error) {
	if !ValidateEmail(email) {
		return nil, errInvalidEmail
	}
	return &User{Email: email, Hash: HashPassword(password)}, nil
}
