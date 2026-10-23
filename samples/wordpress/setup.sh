#!/bin/sh
# Download WordPress and the SQLite Database Integration plugin into
# samples/wordpress/site/ and write a wp-config.php that uses SQLite.
#
# Usage: samples/wordpress/setup.sh [--force]
#
# Environment:
#   WP_VERSION      WordPress version to fetch (default: latest)
#   SQLITE_VERSION  plugin version to fetch (default: latest-stable)
#   WP_DEBUG        1 (default) enables WP_DEBUG + WP_DEBUG_LOG, 0 disables
#   OFFLINE         1 blocks WordPress' outbound HTTP requests (update
#                   checks...); otherwise they go through $https_proxy if set
#
# Downloads honour the usual http_proxy/https_proxy variables (curl reads
# them itself).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
SITE="$HERE/site"
CACHE="$HERE/.cache"
WP_VERSION=${WP_VERSION:-latest}
SQLITE_VERSION=${SQLITE_VERSION:-latest-stable}
WP_DEBUG=${WP_DEBUG:-1}

if [ "${1:-}" = "--force" ]; then
    rm -rf "$SITE"
fi
if [ -e "$SITE/wp-config.php" ]; then
    echo "WordPress already set up in $SITE (use --force to start over)"
    exit 0
fi

mkdir -p "$CACHE"
fetch() {
    # fetch URL FILE: download once into the cache
    if [ ! -s "$CACHE/$2" ]; then
        echo "Downloading $1"
        curl -sSfL --retry 3 -o "$CACHE/$2.part" "$1"
        mv "$CACHE/$2.part" "$CACHE/$2"
    fi
}

if [ "$WP_VERSION" = latest ]; then
    fetch https://wordpress.org/latest.tar.gz wordpress-latest.tar.gz
    WP_TARBALL=wordpress-latest.tar.gz
else
    fetch "https://wordpress.org/wordpress-$WP_VERSION.tar.gz" "wordpress-$WP_VERSION.tar.gz"
    WP_TARBALL="wordpress-$WP_VERSION.tar.gz"
fi
fetch "https://downloads.wordpress.org/plugin/sqlite-database-integration.$SQLITE_VERSION.zip" \
    "sqlite-database-integration.$SQLITE_VERSION.zip"

rm -rf "$SITE.tmp"
mkdir -p "$SITE.tmp"
tar -xzf "$CACHE/$WP_TARBALL" -C "$SITE.tmp"
mv "$SITE.tmp/wordpress" "$SITE"
rmdir "$SITE.tmp"

# The plugin, and its db.php drop-in (db.copy works without the
# placeholder substitution the plugin's activation would do: it falls back
# to wp-content/plugins/sqlite-database-integration).
unzip -q "$CACHE/sqlite-database-integration.$SQLITE_VERSION.zip" -d "$SITE/wp-content/plugins"
cp "$SITE/wp-content/plugins/sqlite-database-integration/db.copy" "$SITE/wp-content/db.php"
mkdir -p "$SITE/wp-content/database"

# WordPress only offers pretty permalinks when it recognises the web
# server (Apache mod_rewrite, nginx, Caddy, IIS). goro-httpd routes
# requests for missing files to /index.php like WordPress' .htaccess does.
mkdir -p "$SITE/wp-content/mu-plugins"
cat > "$SITE/wp-content/mu-plugins/goro-httpd.php" <<'PHP'
<?php
/**
 * Plugin Name: goro-httpd support
 * Description: goro-httpd sends requests for missing files to index.php, so URL rewriting is available.
 */
add_filter( 'got_url_rewrite', '__return_true' );
PHP

salt() {
    # 64 printable characters that need no escaping in a PHP single-quoted string
    LC_ALL=C tr -dc 'A-Za-z0-9!#%()*+,-./:;<=>?@[]^_{|}~' < /dev/urandom | head -c 64
}

if [ "$WP_DEBUG" = 1 ]; then DEBUG=true; else DEBUG=false; fi

# Outbound HTTP (update checks, translations, ...): through the proxy
# from the environment, or blocked altogether with OFFLINE=1.
PROXY_CONF=
proxy=${https_proxy:-${HTTPS_PROXY:-${http_proxy:-${HTTP_PROXY:-}}}}
if [ "${OFFLINE:-0}" = 1 ]; then
    PROXY_CONF="define( 'WP_HTTP_BLOCK_EXTERNAL', true );"
elif [ -n "$proxy" ]; then
    hostport=${proxy#*://}
    hostport=${hostport%%/*}
    hostport=${hostport##*@}
    PROXY_CONF="define( 'WP_PROXY_HOST', '${hostport%:*}' );
define( 'WP_PROXY_PORT', '${hostport##*:}' );
define( 'WP_PROXY_BYPASS_HOSTS', 'localhost, 127.0.0.1' );"
fi

cat > "$SITE/wp-config.php" <<EOF
<?php
/**
 * WordPress configuration for the goro-httpd sample: SQLite via the
 * SQLite Database Integration drop-in (wp-content/db.php).
 */

// The DB_* MySQL settings are unused with the SQLite drop-in.
define( 'DB_NAME', 'wordpress' );
define( 'DB_USER', '' );
define( 'DB_PASSWORD', '' );
define( 'DB_HOST', '' );
define( 'DB_CHARSET', 'utf8mb4' );
define( 'DB_COLLATE', '' );

// SQLite database: wp-content/database/.ht.sqlite (the server refuses to
// serve dotfiles).
define( 'DB_DIR', __DIR__ . '/wp-content/database/' );
define( 'DB_FILE', '.ht.sqlite' );

define( 'AUTH_KEY',         '$(salt)' );
define( 'SECURE_AUTH_KEY',  '$(salt)' );
define( 'LOGGED_IN_KEY',    '$(salt)' );
define( 'NONCE_KEY',        '$(salt)' );
define( 'AUTH_SALT',        '$(salt)' );
define( 'SECURE_AUTH_SALT', '$(salt)' );
define( 'LOGGED_IN_SALT',   '$(salt)' );
define( 'NONCE_SALT',       '$(salt)' );

\$table_prefix = 'wp_';

// The site answers on both the HTTP and the HTTPS port: derive the URLs
// from the request instead of the ones stored at install time.
if ( isset( \$_SERVER['HTTP_HOST'] ) ) {
	\$scheme = ( ! empty( \$_SERVER['HTTPS'] ) && 'off' !== \$_SERVER['HTTPS'] ) ? 'https' : 'http';
	define( 'WP_HOME', \$scheme . '://' . \$_SERVER['HTTP_HOST'] );
	define( 'WP_SITEURL', \$scheme . '://' . \$_SERVER['HTTP_HOST'] );
	unset( \$scheme );
}

define( 'WP_DEBUG', $DEBUG );
define( 'WP_DEBUG_LOG', $DEBUG ? __DIR__ . '/wp-content/debug.log' : false );
define( 'WP_DEBUG_DISPLAY', false );
define( 'WP_ENVIRONMENT_TYPE', 'local' );
define( 'AUTOMATIC_UPDATER_DISABLED', true );
${PROXY_CONF}
define( 'DISABLE_WP_CRON', true );

if ( ! defined( 'ABSPATH' ) ) {
	define( 'ABSPATH', __DIR__ . '/' );
}

require_once ABSPATH . 'wp-settings.php';
EOF

echo "WordPress $(sed -n "s/^\$wp_version = '\(.*\)';/\1/p" "$SITE/wp-includes/version.php") is ready in $SITE"
echo "Start the server with: $HERE/run.sh"
