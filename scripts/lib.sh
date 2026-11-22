# Shared by the scripts (sourced, `. "$ROOT/scripts/lib.sh"`): what differs between Windows (Git Bash / MSYS2) and Linux / macOS.
#
#   winpath <path>   a path as the editor takes it on its command line or in WCE_* variables: Windows form (C:\...) under Git Bash,
#                    unchanged elsewhere
#   mixpath <path>   a path written into an editor script (scripts/*.txt): forward slashes (C:/...) under Git Bash, unchanged elsewhere
#   EXE              the executables' suffix: ".exe" on Windows, empty elsewhere ("$BIN/wce$EXE")
#   SEP              the separator of path lists in environment variables: ';' on Windows, ':' elsewhere
#   PY               the Python 3 interpreter: `python` (Windows' python3 is often the Store's stub), else `python3`
#   SETTINGS_SRC     the folder of the user's settings.toml, copied into the scripts' throw-away settings: $WCE_SETTINGS_DIR, else the
#                    editor's own (wce_foundation::settings::Paths::standard)
#   GAME             the game folder: $WCE_GAME, else the current game's `dir` in settings.toml, else Steam's default library
#   WCE_GPU          exported only when set, or on Windows, where the reference pixels come from one GPU (docs/baseline.md)
#   wce_timeout <s>  `timeout <s>` where there is one (macOS has none unless coreutils' gtimeout), else the command as it is

case "${OSTYPE:-}" in
    msys* | cygwin* | win*) WCE_WINDOWS=1 ;;
    *) WCE_WINDOWS=0 ;;
esac

if [ "$WCE_WINDOWS" = 1 ]; then
    EXE=.exe
    SEP=';'
    winpath() { cygpath -w "$1"; }
    mixpath() { cygpath -m "$1"; }
    SETTINGS_SRC="${WCE_SETTINGS_DIR:-$LOCALAPPDATA/wce/config}"
    _wce_steam_game="D:/SteamLibrary/steamapps/common/Total War Attila"
    export WCE_GPU="${WCE_GPU:-AMD Radeon}"
else
    EXE=
    SEP=':'
    winpath() { printf '%s\n' "$1"; }
    mixpath() { printf '%s\n' "$1"; }
    case "${OSTYPE:-}" in
        darwin*)
            SETTINGS_SRC="${WCE_SETTINGS_DIR:-$HOME/Library/Application Support/wce}"
            _wce_steam_game="$HOME/Library/Application Support/Steam/steamapps/common/Total War Attila"
            ;;
        *)
            SETTINGS_SRC="${WCE_SETTINGS_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/wce}"
            _wce_steam_game="$HOME/.local/share/Steam/steamapps/common/Total War Attila"
            ;;
    esac
    [ -n "${WCE_GPU:-}" ] && export WCE_GPU
fi

if command -v python > /dev/null 2>&1; then PY=python; else PY=python3; fi

# A value of settings.toml: `toml_value <key>` the first top-level `<key> = "..."`, `toml_value <key> <table>` the one in `[<table>]`.
toml_value() {
    awk -v key="$1" -v table="${2:-}" '
        /^\[/ { t = $0; gsub(/^\[+|\]+$/, "", t); next }
        t == table && $0 ~ "^" key " *= *" {
            v = $0; sub("^" key " *= *", "", v); gsub(/^["\x27]|["\x27] *$/, "", v); print v; exit
        }' "$SETTINGS_SRC/settings.toml" 2> /dev/null
}

if [ -z "${WCE_GAME:-}" ]; then
    _wce_game_key="$(toml_value game)"
    GAME="$(toml_value dir "games.${_wce_game_key:-attila}")"
    GAME="${GAME:-$_wce_steam_game}"
else
    GAME="$WCE_GAME"
fi

wce_timeout() {
    local s="$1"
    shift
    if command -v timeout > /dev/null 2>&1; then
        timeout "$s" "$@"
    elif command -v gtimeout > /dev/null 2>&1; then
        gtimeout "$s" "$@"
    else
        "$@"
    fi
}
