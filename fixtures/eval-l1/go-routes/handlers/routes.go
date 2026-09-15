package handlers

import "net/http"

func GetUsers(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("[]"))
}

func CreateUser(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("{}"))
}

func Healthz(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("ok"))
}

// Route table — classic Go DI map (Heuristic).
var Routes = map[string]http.HandlerFunc{
	"/users":  GetUsers,
	"/create": CreateUser,
	"/health": Healthz,
}

func Register(mux *http.ServeMux) {
	for path, h := range Routes {
		mux.HandleFunc(path, h)
	}
}
