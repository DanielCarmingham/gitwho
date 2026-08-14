# Deploying gitwho.cc

The site ships as its own container to the uncloud cluster: a multi-stage
image builds the static Astro output with Node, then serves it from
`caddy:2-alpine`, with the cluster's ingress Caddy terminating TLS and routing
by hostname to this container's plain `:80`.

## The one surprising thing: the build context is the repo root, not `site/`

The site reads its documentation **in place** from the gitwho repo root at
build time — `../docs/INSTALL.md`, `../docs/DESIGN.md`,
`../docs/accounts.toml.example`, `../SECURITY.md` (see
`site/src/content.config.ts` and `site/src/docs-map.ts`), and
`npm run build` also runs the test suite first, which additionally reads
`../src/config.rs` (`tests/recipes.test.ts` derives the `accounts.toml` schema
from the Rust struct definitions rather than trusting the example to be
exhaustive). All of that is *outside* `site/`.

So the Docker build context must be the **repository root**, with
`dockerfile: site/Dockerfile` — not `site/`, and not the default. `compose.yaml`
sets this (`build.context: ..`). The Dockerfile copies `docs/`, `SECURITY.md`
and `src/config.rs` from the repo root alongside `site/`, then builds from
`/app/site` with `/app` playing the part of the repo root — the same relative
layout (`docs/`, `SECURITY.md`, `src/config.rs` one level above the Astro
project) the site expects when built from a developer's checkout. Get this
wrong (context of just `site/`) and `npm run build` fails inside
`assertSourcesPresent` or `recipes.test.ts`, not at deploy time but partway
through the build — this is the single most surprising thing about this setup
and the thing most likely to waste an afternoon.

**A consequence of the same fact:** the `.dockerignore` lives at the **repo
root**, not in `site/`. Docker only honours a `.dockerignore` located at the
build context root; with the context at the repo root, a `site/.dockerignore`
(or even a `site/Dockerfile.dockerignore`) is silently ignored. Confirmed by
building both ways — see below.

## Deploy command

```bash
cd site
uc deploy -f compose.yaml --recreate
uc ls
```

**`--recreate` is not optional, and leaving it off fails silently.** The image
is tagged `latest`, so pushing new layers changes neither the tag nor the
service config — `uc` compares those, sees no difference, prints
`Services are up to date`, and leaves the old container running. The deploy
reports success and the site does not change.

Nothing in the verification below catches it either: status codes, the redirect
direction and both cache headers all pass perfectly against a stale container.
The only check that distinguishes a deploy that landed from one that did not is
comparing what is actually being served against what was just built:

```bash
# the asset hash the local build produced
grep -o '/_astro/[A-Za-z0-9._-]*\.css' site/dist/index.html | head -1

# what the live site is serving — these must match
curl -s https://www.gitwho.cc/ | grep -o '/_astro/[A-Za-z0-9._-]*\.css' | head -1
```

Astro fingerprints those filenames from content, so equal hashes mean the
running container is built from the same source. Observed 2026-08-14: a deploy
without `--recreate` left `BaseLayout.DGM8XPJU.css` serving while the build had
produced `BaseLayout.DzNA6BMd.css`.

## Verify — local image, before deploying

```bash
cd <repo root>
docker build --platform linux/amd64 -f site/Dockerfile -t gitwho-cc:test .
docker run --rm -d -p 8090:80 --name gitwho-cc-test gitwho-cc:test

curl -sI http://localhost:8090/                          # expect 200
curl -sI http://localhost:8090/docs/recipes/              # expect 200
curl -sI http://localhost:8090/docs/config/                # expect 200
curl -sI -H 'Host: gitwho.cc' http://localhost:8090/       # expect 301 -> https://www.gitwho.cc/
curl -sI -H 'Host: www.gitwho.cc' http://localhost:8090/   # expect 200, NOT a redirect
curl -s -D- -o /dev/null http://localhost:8090/ | grep -i cache-control            # max-age=0, must-revalidate
curl -s -D- -o /dev/null http://localhost:8090/_astro/<some-asset> | grep -i cache-control  # immutable

docker rm -f gitwho-cc-test
```

The two `Host:`-header checks are the ones that matter most: they prove the
redirect direction (apex → www, not the reverse) before it can affect anyone.
A naive check of `/` alone would return 200 either way and hide a backwards
redirect until it hit production.

## Verify — live site, after deploying

```bash
curl -sI https://www.gitwho.cc/  | head -n 1        # expect 200
curl -sI http://gitwho.cc/       | head -n 3        # expect 301 -> https://www.gitwho.cc
curl -sv https://www.gitwho.cc/ 2>&1 | grep -i 'issuer\|subject'
curl -s -D- -o /dev/null https://www.gitwho.cc/_astro/<some-asset> | grep -i cache-control
```

Let's Encrypt issues a certificate only after a hostname resolves to the
cluster, so the first requests after deploy may fail for a few minutes. Retry
before diagnosing.

## DNS requirement

Both `gitwho.cc` and `www.gitwho.cc` must resolve to the cluster, and
**neither may be proxied** (grey cloud / "DNS only" on Cloudflare). A proxied
record means Cloudflare terminates TLS at its edge; the cluster ingress cannot
then complete an ACME (Let's Encrypt) challenge, and under Cloudflare's
"Flexible" SSL mode the site's own HTTPS redirect becomes a loop.

The apex is listed in `compose.yaml`'s `x-ports` alongside `www` even though it
only redirects: a browser validates TLS before it ever sees a redirect, so the
apex needs its own certificate, and it only gets one by being routed to the
container.

## Canonical host

**www-canonical**, matching every other site on this cluster: the apex 301s to
`https://www.gitwho.cc`, and `www` serves the files. This is the mirror image
of `throughline`'s Caddyfile, which is apex-canonical — `throughline` runs on
its own separate stack, so it neither follows nor breaks this cluster's
convention.

## The amd64 pin

The cluster runs `linux/amd64`. Both `platforms: [linux/amd64]` (build) and
`platform: linux/amd64` (run) are set in `compose.yaml`. Without them, a build
from an Apple Silicon machine produces an `arm64` image that dies with
`exec format error` on the cluster. Building and running locally on Apple
Silicon still works via QEMU emulation (Docker Desktop prints a platform
mismatch warning on `docker run`, which is expected and harmless for local
verification).

## Cache headers

- `/_astro/*` — `public, max-age=31536000, immutable`. Astro fingerprints
  these filenames, so a new deploy always ships new URLs.
- Everything else (all HTML pages, `favicon.ico`, `favicon.svg`) —
  `public, max-age=0, must-revalidate`. Matched by **excluding** `/_astro/*`
  rather than by matching a `.html` extension: `trailingSlash: 'always'` means
  every page is requested as a directory (`/docs/recipes/`), not literally as
  `*.html`, so an extension-based matcher silently misses every page but the
  root and ships no `Cache-Control` header at all on the rest. Confirmed by
  curling a sub-page against the first draft of the Caddyfile before fixing
  it.

## Measured build-context size

`docker build` context size before and after the repo-root `.dockerignore`
(measured with the classic builder, `DOCKER_BUILDKIT=0`, for a clean
full-context comparison — BuildKit's default builder does differential/
sparse context transfer and is not a fair apples-to-apples number):

| | Context size |
|---|---|
| Before `.dockerignore` | 5.138 GB |
| After `.dockerignore` | 1.443 MB |

Without it, the context is the whole gitwho repo including `target/` (2.8 GB)
and `target.noindex/` (1.9 GB) — the Rust build output has nothing to do with
this site but sits right next to it at the repo root.
