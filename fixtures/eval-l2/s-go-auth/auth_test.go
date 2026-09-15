package sgo

import "testing"

func TestAuthenticate(t *testing.T) {
	if Authenticate("a@b.com", "x") == nil {
		t.Fatal("expected user")
	}
	if Authenticate("", "x") != nil {
		t.Fatal("expected nil")
	}
}
