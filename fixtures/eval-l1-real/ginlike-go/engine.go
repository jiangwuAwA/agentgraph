package ginlike

import "net/http"

type HandlerFunc func(w http.ResponseWriter, r *http.Request)

type Engine struct {
	routes map[string]HandlerFunc
}

func New() *Engine {
	return &Engine{routes: map[string]HandlerFunc{}}
}

func (e *Engine) GET(path string, h HandlerFunc) {
	e.routes[path] = h
}

func GetUsers(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("[]"))
}

func Healthz(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("ok"))
}

func Bootstrap() *Engine {
	e := New()
	e.GET("/users", GetUsers)
	e.GET("/health", Healthz)
	return e
}
