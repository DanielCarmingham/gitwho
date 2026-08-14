# Build the static site, then serve it from a tiny Caddy image.
# The cluster's Caddy terminates TLS and routes by hostname, so this one
# only ever speaks plain HTTP on :80.
#
# The build context is the REPO ROOT, not site/ -- see compose.yaml. The site
# reads its documentation in place from the repo root (../docs/INSTALL.md,
# ../docs/DESIGN.md, ../docs/accounts.toml.example, ../SECURITY.md; see
# site/src/content.config.ts and site/src/docs-map.ts, which resolve those
# paths as `base: '..'` / `../${file}`). `npm run build` also runs the test
# suite first (see package.json), and tests/recipes.test.ts additionally reads
# ../src/config.rs -- it derives the accounts.toml schema from the Rust struct
# definitions rather than trusting the example to be exhaustive. A context of
# just site/ builds an image whose `npm run build` fails inside
# assertSourcesPresent (missing docs) or recipes.test.ts (missing config.rs)
# because those files are outside it. This Dockerfile copies the repo root's
# docs/, SECURITY.md and src/config.rs alongside site/, and runs the build
# from /app/site with /app playing the part of the repo root -- the same
# relative layout the site expects when built from a developer's checkout.

FROM node:22-alpine AS build
WORKDIR /app/site
COPY site/package.json site/package-lock.json ./
RUN npm ci
WORKDIR /app
COPY docs ./docs
COPY SECURITY.md ./SECURITY.md
COPY src/config.rs ./src/config.rs
COPY site ./site
WORKDIR /app/site
RUN npm run build

FROM caddy:2-alpine
COPY site/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /app/site/dist /srv
EXPOSE 80
