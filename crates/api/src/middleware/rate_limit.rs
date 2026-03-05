// Rate limiting is not enforced in the self-hosted OSS binary.
// Operators who need ingress rate limiting should use a reverse proxy
// (Nginx, Caddy, Traefik, etc.).
//
// The cloud binary adds its own plan-aware rate limiting via `struxio-cloud`.
