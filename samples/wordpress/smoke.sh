#!/bin/bash
# End-to-end smoke test of a fresh WordPress site (set up by setup.sh and
# served by run.sh): installer, front page, login, dashboard, permalinks,
# publishing through the REST API, comments, uploads, static files and
# HTTPS. Exits non-zero on the first failure.
#
# Usage: samples/wordpress/smoke.sh [HTTP_BASE] [HTTPS_BASE]
#   HTTP_BASE   default http://127.0.0.1:8080
#   HTTPS_BASE  default https://127.0.0.1:8443 ("" skips the HTTPS checks)
#
# Environment:
#   CHECK_DOTFILES  0 skips the check that dotfiles (the SQLite database)
#               are not served (php -S serves them)
#   SMOKE_FAIL_BODY  file to save the failing response body to
#   DEBUG_LOG   path of WP_DEBUG_LOG to check for PHP diagnostics
#               (default: site/wp-content/debug.log next to this script)
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
HTTP=${1:-http://127.0.0.1:8080}
HTTPS=${2-https://127.0.0.1:8443}
DEBUG_LOG=${DEBUG_LOG:-$HERE/site/wp-content/debug.log}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
JAR="$WORK/cookies"
BODY="$WORK/body"
HEAD="$WORK/headers"

ADMIN_USER=admin
ADMIN_PASS='goro-Sample-pw-1!'
SLUG="goro-smoke-$(date +%s)"

pass=0
ok() { pass=$((pass + 1)); printf '  ok   %s\n' "$*"; }
fail() {
    printf '  FAIL %s\n' "$*"
    if [ -n "${SMOKE_FAIL_BODY:-}" ]; then cp "$BODY" "$SMOKE_FAIL_BODY"; fi
    if [ -s "$HEAD" ]; then sed 's/^/       > /' "$HEAD" | head -20; fi
    if [ -s "$BODY" ]; then head -c 1500 "$BODY" | sed 's/^/       | /'; echo; fi
    exit 1
}

# req METHOD URL [curl args...]: sets $STATUS, $TIME; body in $BODY, headers in $HEAD
req() {
    local method=$1 url=$2
    shift 2
    local out
    out=$(curl -s -k --noproxy '*' -X "$method" -b "$JAR" -c "$JAR" -D "$HEAD" -o "$BODY" \
        -w '%{http_code} %{time_total}' "$@" "$url") || { STATUS=000; TIME=0; return; }
    STATUS=${out% *}
    TIME=${out#* }
}

expect_status() {
    [ "$STATUS" = "$1" ] || fail "$2: expected HTTP $1, got $STATUS"
}
expect_body() {
    grep -q -- "$1" "$BODY" || fail "$2: response does not contain '$1'"
}
expect_header() {
    grep -qi -- "$1" "$HEAD" || fail "$2: no header matching '$1'"
}
# No PHP diagnostics in a page
expect_clean() {
    if grep -Eq '<b>(Warning|Notice|Deprecated|Fatal error)</b>:|^(PHP )?(Warning|Notice|Deprecated|Fatal error): ' "$BODY"; then
        fail "$1: PHP diagnostics in the output"
    fi
}
# form_field NAME: the value of an <input name="NAME"> in $BODY
form_field() {
    grep -o "name=\"$1\" value=\"[^\"]*\"" "$BODY" | head -1 | sed 's/.*value="\([^"]*\)"/\1/'
}

echo "WordPress smoke test against $HTTP${HTTPS:+ and $HTTPS}"

# --- installer -------------------------------------------------------------
req GET "$HTTP/"
[ "$STATUS" = 000 ] && fail "cannot connect to $HTTP"
if [ "$STATUS" = 302 ] && grep -qi '^location:.*wp-admin/install.php' "$HEAD"; then
    ok "uninstalled site redirects to the installer"
    req GET "$HTTP/wp-admin/install.php"
    expect_status 200 "installer page"
    expect_clean "installer page"
    if grep -q 'language-chooser' "$BODY"; then
        # Translations could be fetched from wordpress.org: pick English
        ok "installer language chooser"
        req POST "$HTTP/wp-admin/install.php?step=1" --data "language="
        expect_status 200 "installer page"
    fi
    expect_body 'name="weblog_title"' "installer page"
    expect_clean "installer page"
    ok "installer form"
    req POST "$HTTP/wp-admin/install.php?step=2" \
        --data-urlencode "weblog_title=goro WordPress" \
        --data-urlencode "user_name=$ADMIN_USER" \
        --data-urlencode "admin_password=$ADMIN_PASS" \
        --data-urlencode "admin_password2=$ADMIN_PASS" \
        --data-urlencode "pw_weak=1" \
        --data-urlencode "admin_email=admin@example.com" \
        --data-urlencode "blog_public=0" \
        --data-urlencode "Submit=Install WordPress" \
        --data-urlencode "language="
    expect_status 200 "installation"
    expect_body 'Success!' "installation"
    expect_clean "installation"
    ok "installation (step 2) in ${TIME}s"
else
    ok "site already installed (HTTP $STATUS)"
fi

# --- front page ------------------------------------------------------------
req GET "$HTTP/"
expect_status 200 "front page"
expect_body 'Hello world!' "front page"
expect_body '<html' "front page"
expect_clean "front page"
expect_header '^content-type: text/html; charset=UTF-8' "front page"
ok "front page shows 'Hello world!' (${TIME}s)"

# --- static files ----------------------------------------------------------
req GET "$HTTP/wp-includes/css/dashicons.min.css?ver=1"
expect_status 200 "static CSS"
expect_header '^content-type: text/css' "static CSS"
ok "static CSS with Content-Type text/css"
req GET "$HTTP/wp-includes/js/jquery/jquery.min.js"
expect_status 200 "static JS"
expect_header '^content-type: .*javascript' "static JS"
ok "static JS"
if [ "${CHECK_DOTFILES:-1}" = 1 ]; then
    req GET "$HTTP/wp-content/database/.ht.sqlite"
    [ "$STATUS" = 200 ] && fail "the SQLite database is downloadable"
    ok "SQLite database file is not served (HTTP $STATUS)"
fi

# --- login -----------------------------------------------------------------
req GET "$HTTP/wp-login.php"
expect_status 200 "login page"
expect_body 'name="log"' "login page"
expect_clean "login page"
ok "login form"
req POST "$HTTP/wp-login.php" \
    --data-urlencode "log=$ADMIN_USER" \
    --data-urlencode "pwd=$ADMIN_PASS" \
    --data-urlencode "wp-submit=Log In" \
    --data-urlencode "redirect_to=$HTTP/wp-admin/" \
    --data-urlencode "testcookie=1"
expect_status 302 "login"
expect_header '^location: .*/wp-admin/' "login"
expect_header '^set-cookie: wordpress_logged_in_' "login"
ok "login sets the auth cookies and redirects to /wp-admin/"

# --- dashboard -------------------------------------------------------------
req GET "$HTTP/wp-admin/"
expect_status 200 "dashboard"
expect_body '<title>Dashboard' "dashboard"
expect_clean "dashboard"
ok "dashboard (${TIME}s)"

# --- permalinks ------------------------------------------------------------
req GET "$HTTP/wp-admin/options-permalink.php"
expect_status 200 "permalink settings"
expect_clean "permalink settings"
NONCE=$(form_field _wpnonce)
[ -n "$NONCE" ] || fail "permalink settings: no _wpnonce"
req POST "$HTTP/wp-admin/options-permalink.php" \
    --data-urlencode "_wpnonce=$NONCE" \
    --data-urlencode "_wp_http_referer=/wp-admin/options-permalink.php" \
    --data-urlencode "selection=/%postname%/" \
    --data-urlencode "permalink_structure=/%postname%/" \
    --data-urlencode "category_base=" \
    --data-urlencode "tag_base=" \
    --data-urlencode "submit=Save Changes"
expect_status 302 "save permalinks"
req GET "$HTTP/wp-admin/options-permalink.php?settings-updated=true"
grep -A1 'type="radio" value="/%postname%/"' "$BODY" | grep -q checked || fail "permalink structure not saved"
ok "permalink structure set to /%postname%/"

# --- publish a post through the REST API -----------------------------------
req GET "$HTTP/wp-admin/admin-ajax.php?action=rest-nonce"
expect_status 200 "REST nonce"
REST_NONCE=$(cat "$BODY")
[ -n "$REST_NONCE" ] || fail "REST nonce"
req POST "$HTTP/wp-json/wp/v2/posts" \
    -H "X-WP-Nonce: $REST_NONCE" -H 'Content-Type: application/json' \
    --data "{\"title\":\"Hello from goro\",\"slug\":\"$SLUG\",\"status\":\"publish\",\"content\":\"<p>Published by the goro smoke test.</p>\"}"
expect_status 201 "create post"
expect_body "\"slug\":\"$SLUG\"" "create post"
POST_ID=$(grep -o '"id":[0-9]*' "$BODY" | head -1 | cut -d: -f2)
ok "post $POST_ID published through the REST API"

req GET "$HTTP/$SLUG/"
expect_status 200 "pretty permalink"
expect_body 'Hello from goro' "pretty permalink"
expect_body 'Published by the goro smoke test.' "pretty permalink"
expect_clean "pretty permalink"
ok "post served at its pretty permalink /$SLUG/"

req GET "$HTTP/?p=$POST_ID"
expect_status 301 "plain permalink"
expect_header "^location: $HTTP/$SLUG/" "plain permalink"
ok "?p=$POST_ID redirects to the pretty permalink"

req GET "$HTTP/wp-json/wp/v2/posts/$POST_ID"
expect_status 200 "REST read"
expect_header '^content-type: application/json' "REST read"
ok "REST API read"

req GET "$HTTP/no-such-page-$SLUG/"
expect_status 404 "missing page"
expect_clean "missing page"
ok "missing page gives the theme's 404"

# --- classic form post: a comment ------------------------------------------
req POST "$HTTP/wp-comments-post.php" \
    --data-urlencode "comment=A comment from the goro smoke test." \
    --data-urlencode "comment_post_ID=$POST_ID" \
    --data-urlencode "comment_parent=0"
expect_status 302 "comment"
req GET "$HTTP/$SLUG/"
expect_body 'A comment from the goro smoke test.' "comment shown"
ok "comment posted and shown"

# --- multipart upload ------------------------------------------------------
printf 'uploaded by the goro smoke test\n' > "$WORK/goro-upload.txt"
req POST "$HTTP/wp-json/wp/v2/media" -H "X-WP-Nonce: $REST_NONCE" \
    -F "file=@$WORK/goro-upload.txt;type=text/plain"
expect_status 201 "media upload"
MEDIA_IDS=$(grep -o '"id":[0-9]*' "$BODY" | head -1 | cut -d: -f2)
URL=$(grep -o '"source_url":"[^"]*"' "$BODY" | head -1 | sed 's/.*:"\(.*\)"/\1/; s/\\\//\//g')
[ -n "$URL" ] || fail "media upload: no source_url"
req GET "$URL"
expect_status 200 "uploaded file"
expect_body 'uploaded by the goro smoke test' "uploaded file"
ok "multipart upload ($URL)"

req POST "$HTTP/wp-json/wp/v2/media" -H "X-WP-Nonce: $REST_NONCE" \
    -F "file=@$HERE/test-image.jpg;type=image/jpeg"
expect_status 201 "image upload"
MEDIA_IDS="$MEDIA_IDS $(grep -o '"id":[0-9]*' "$BODY" | head -1 | cut -d: -f2)"
grep -q '"thumbnail":{"file":"test-image[-0-9]*-150x150.jpg"' "$BODY" || fail "image upload: no 150x150 thumbnail"
THUMB=$(grep -o '"thumbnail":{[^}]*"source_url":"[^"]*"' "$BODY" | sed 's/.*"source_url":"\(.*\)"/\1/; s/\\\//\//g')
req GET "$THUMB"
expect_status 200 "thumbnail"
expect_header '^content-type: image/jpeg' "thumbnail"
ok "image upload with generated thumbnails ($THUMB)"

# --- HTTPS -----------------------------------------------------------------
if [ -n "$HTTPS" ]; then
    rm -f "$JAR"
    req GET "$HTTPS/"
    expect_status 200 "HTTPS front page"
    expect_body 'Hello world!' "HTTPS front page"
    expect_body "href=\"$HTTPS/" "HTTPS front page links"
    ok "HTTPS front page (${TIME}s)"
    req GET "$HTTPS/$SLUG/"
    expect_status 200 "HTTPS permalink"
    expect_body 'Hello from goro' "HTTPS permalink"
    ok "HTTPS pretty permalink"
    req GET "$HTTPS/wp-login.php"
    req POST "$HTTPS/wp-login.php" \
        --data-urlencode "log=$ADMIN_USER" \
        --data-urlencode "pwd=$ADMIN_PASS" \
        --data-urlencode "wp-submit=Log In" \
        --data-urlencode "redirect_to=$HTTPS/wp-admin/" \
        --data-urlencode "testcookie=1"
    expect_status 302 "HTTPS login"
    expect_header '^set-cookie: wordpress_sec_.*secure' "HTTPS login (secure auth cookie)"
    req GET "$HTTPS/wp-admin/"
    expect_status 200 "HTTPS dashboard"
    expect_body '<title>Dashboard' "HTTPS dashboard"
    ok "HTTPS login and dashboard (secure cookies)"
    if curl -s -k --noproxy '*' --http2 -o /dev/null -w '%{http_version}' "$HTTPS/wp-includes/css/dashicons.min.css" | grep -q '^2'; then
        ok "HTTP/2 over TLS"
    else
        ok "HTTP/2 not negotiated (curl without HTTP/2?)"
    fi
fi

# --- clean up: delete the post (and its comment) and the uploads ----------
# Without this, the "Hello from goro" posts pile up across runs and push
# "Hello world!" off the front page.
BASE=${HTTPS:-$HTTP}
req GET "$BASE/wp-admin/admin-ajax.php?action=rest-nonce"
expect_status 200 "REST nonce (cleanup)"
REST_NONCE=$(cat "$BODY")
req DELETE "$BASE/wp-json/wp/v2/posts/$POST_ID?force=true" -H "X-WP-Nonce: $REST_NONCE"
expect_status 200 "delete post"
for id in $MEDIA_IDS; do
    req DELETE "$BASE/wp-json/wp/v2/media/$id?force=true" -H "X-WP-Nonce: $REST_NONCE"
    expect_status 200 "delete upload $id"
done
ok "test post and uploads deleted through the REST API"

# --- WP_DEBUG_LOG ----------------------------------------------------------
if [ -f "$DEBUG_LOG" ] && [ -s "$DEBUG_LOG" ]; then
    echo "  FAIL $DEBUG_LOG is not empty:"
    head -30 "$DEBUG_LOG" | sed 's/^/       | /'
    exit 1
fi
ok "debug.log is clean"

echo "All $pass checks passed."
