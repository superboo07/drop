#!/bin/bash

# This file starts up the Drop server by running migrations and then starting the executable
#
# Note: the Dockerfile's CMD invokes this with `sh`, not bash, so keep it POSIX.

set -u

MIGRATIONS_DIR="${MIGRATIONS_DIR:-/app/prisma/migrations}"
MIGRATE_LOG=/tmp/drop-migrate.log

run_deploy() {
    pnpm prisma migrate deploy >"$MIGRATE_LOG" 2>&1
    deploy_status=$?
    cat "$MIGRATE_LOG"
    return $deploy_status
}

# Names of the migrations Prisma just reported as failed. It words this two
# different ways depending on when the failure happened:
#
#   P3018, on the run that fails:
#     Migration name: 20260726041153_add_user_groups
#   P3009, on every run afterwards:
#     The `20260726041153_add_user_groups` migration started at ... failed
failed_migrations() {
    sed -n \
        -e 's/^[[:space:]]*Migration name:[[:space:]]*\([^[:space:]]*\).*$/\1/p' \
        -e 's/^[[:space:]]*The `\([^`]*\)` migration started at .* failed[[:space:]]*$/\1/p' \
        "$MIGRATE_LOG" | sort -u
}

echo "[Drop] performing migrations..."
if run_deploy; then
    echo "[Drop] migrations applied"
else
    # A failed migration wedges the database: Prisma refuses to apply anything
    # further until the failure is acknowledged, so without this the server
    # would keep starting against a schema that is missing tables and only fail
    # once a request touches them.
    #
    # Postgres DDL is transactional, so a migration that errors is rolled back
    # in full -- there is no partial state to reconcile, and marking it rolled
    # back is accurate rather than a guess. That stops being true for anything
    # using CONCURRENTLY, which cannot run in a transaction, so bail out and ask
    # for a human if a failed migration contains it.
    echo "[Drop] migrations failed - checking whether this can be recovered automatically"

    names=$(failed_migrations)
    if [ -z "$names" ]; then
        echo "[Drop] no failed migration was named, so there is nothing to recover from."
        echo "[Drop] refusing to start against a database in an unknown state."
        exit 1
    fi

    for name in $names; do
        sql="$MIGRATIONS_DIR/$name/migration.sql"
        if [ -f "$sql" ] && grep -qi 'CONCURRENTLY' "$sql"; then
            echo "[Drop] $name uses CONCURRENTLY, so it cannot have been rolled back atomically."
            echo "[Drop] this needs to be resolved by hand:"
            echo "[Drop]   pnpm prisma migrate resolve --rolled-back $name"
            exit 1
        fi

        echo "[Drop] marking failed migration $name as rolled back"
        if ! pnpm prisma migrate resolve --rolled-back "$name"; then
            echo "[Drop] could not mark $name as rolled back - refusing to start."
            exit 1
        fi
    done

    echo "[Drop] retrying migrations..."
    if ! run_deploy; then
        echo "[Drop] migrations still failing after recovery - refusing to start."
        echo "[Drop] the database needs attention before Drop can run."
        exit 1
    fi
    echo "[Drop] migrations recovered"
fi

# Actually start the application
node /app/app/server/index.mjs
