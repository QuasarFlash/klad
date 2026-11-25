#!/usr/bin/env bash
title="${1:?give the annotation title}"
log="${2:?give the log file}"
text="$(sed 's/\x1b\[[0-9;]*[A-Za-z]//g' "$log" 2>/dev/null | tail -c 3500)"
[ -n "$text" ] || text="nothing in $log"
text="${text//'%'/%25}"
text="${text//$'\r'/%0D}"
text="${text//$'\n'/%0A}"
echo "::error title=$title::$text"
