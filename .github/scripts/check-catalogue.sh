#!/usr/bin/env bash
set -eu

# Run from the repository root. Check Cargo names, not Rust module spellings.
catalogue=docs/CATALOGUE.md
status=0
shopt -s nullglob dotglob
for dir in crates/*/; do
    name=${dir%/}
    name=${name##*/}
    if ! grep -Fq -- "\`$name\`" "$catalogue"; then
        printf '%s: missing crate `%s`\n' "$catalogue" "$name" >&2
        status=1
    fi
done

# Read inline Markdown destinations only in the index. Leave URLs and anchors alone.
if ! links=$(awk '
    { sub(/\r$/, "") }
    /^## I want to \.\.\.$/ { found = 1; next }
    found && /^#{1,2} / { exit }
    found {
        while (match($0, /\]\([^)]*\)/)) {
            print substr($0, RSTART + 2, RLENGTH - 3)
            $0 = substr($0, RSTART + RLENGTH)
        }
    }
    END { if (!found) exit 1 }
' "$catalogue"); then
    printf '%s: could not read the "I want to ..." index\n' "$catalogue" >&2
    exit 1
fi

while IFS= read -r link; do
    # Let Markdown links carry an optional title, or enclose a path in angle brackets.
    link=${link#"${link%%[![:space:]]*}"}
    case "$link" in
        '<'*) link=${link#<}; link=${link%%>*} ;;
        *) link=${link%%[[:space:]]*} ;;
    esac
    case "$link" in
        ''|'#'*|//*|*://*|mailto:*) continue ;;
    esac
    path=${link%%[?#]*}
    case "$path" in
        '') continue ;;
        /*) path=${path#/} ;;
        *) path=${catalogue%/*}/$path ;;
    esac
    if [[ ! -e "$path" ]]; then
        printf '%s: index links to missing path `%s`\n' "$catalogue" "$path" >&2
        status=1
    fi
done <<< "$links"

exit "$status"
