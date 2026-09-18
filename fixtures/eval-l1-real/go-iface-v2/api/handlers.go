// Real-idiom multi-file Go: consumer of the interface.

package api

import "example.com/app/store"

func LoadUser(r store.Repository, id string) string {
	return r.Get(id)
}

func SaveUser(r store.Repository, id, body string) {
	r.Put(id, body)
}
