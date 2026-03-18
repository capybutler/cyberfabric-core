#!/usr/bin/env python3
"""Fetch GitHub repo activity stats via `gh` CLI and output a Markdown report.

Usage:
    python scripts/gh_stats.py [--since YYYY-MM-DD] [--until YYYY-MM-DD] \
        [--exclude-labels pr-issue,wontfix] [owner/repo]

Default repo: cyberfabric/cyberfabric-core
Requires: gh CLI installed and authenticated.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from statistics import median
from typing import Any

DEFAULT_REPO = "cyberfabric/cyberfabric-core"
DEFAULT_EXCLUDE_LABELS = ["pr-issue"]

# Accounts to filter out of human tables
BOTS = {
    "github-actions[bot]",
    "coderabbitai[bot]",
    "qodo-code-review[bot]",
    "mergify[bot]",
    "codecov-commenter",
    "graphite-app[bot]",
    "dependabot[bot]",
    "renovate[bot]",
    "Copilot",
    "github-code-quality[bot]",
}


def gh_api_paginate(endpoint: str, repo: str) -> list[dict[str, Any]]:
    """Call `gh api` with pagination and return parsed JSON list."""
    cmd = [
        "gh", "api",
        f"repos/{repo}/{endpoint}",
        "--paginate",
        "--jq", ".",
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, check=True)
    raw = result.stdout.strip()
    if not raw:
        return []
    raw = raw.replace("]\n[", ",").replace("][", ",")
    return json.loads(raw)


def parse_iso(ts: str) -> datetime:
    """Parse GitHub ISO-8601 timestamp."""
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


def filter_by_date(
    items: list[dict],
    since: datetime | None,
    until: datetime | None,
    date_field: str = "created_at",
) -> list[dict]:
    if not since and not until:
        return items
    result = []
    for item in items:
        ts = item.get(date_field)
        if not ts:
            continue
        dt = parse_iso(ts)
        if since and dt < since:
            continue
        if until and dt > until:
            continue
        result.append(item)
    return result


def is_bot(login: str) -> bool:
    return login in BOTS or login.endswith("[bot]")


def has_excluded_label(item: dict, exclude_labels: set[str]) -> bool:
    labels = {lbl["name"] for lbl in item.get("labels", [])}
    return bool(labels & exclude_labels)


def aggregate(items: list[dict], user_key: str = "user", body_key: str = "body"):
    """Return {login: [count, total_bytes]} excluding bots."""
    stats: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for item in items:
        login = item.get(user_key, {}).get("login", "unknown")
        if is_bot(login):
            continue
        body_len = len(item.get(body_key) or "")
        stats[login][0] += 1
        stats[login][1] += body_len
    return stats


def fmt_kb(b: int) -> str:
    if b < 1024:
        return f"{b} B"
    return f"{b / 1024:.1f} KB"


def fmt_duration(td: timedelta) -> str:
    """Format timedelta as human-readable string."""
    total_seconds = int(td.total_seconds())
    if total_seconds < 0:
        return "N/A"
    days = total_seconds // 86400
    hours = (total_seconds % 86400) // 3600
    if days > 0:
        return f"{days}d {hours}h"
    minutes = (total_seconds % 3600) // 60
    if hours > 0:
        return f"{hours}h {minutes}m"
    return f"{minutes}m"


def md_table(title: str, stats: dict[str, list[int]], sort_col: int = 0) -> str:
    rows = sorted(stats.items(), key=lambda x: x[1][sort_col], reverse=True)
    if not rows:
        return f"## {title}\n\nNo data.\n"
    lines = [
        f"## {title}\n",
        "| # | User | Count | Text volume |",
        "|--:|------|------:|------------:|",
    ]
    for i, (login, (count, size)) in enumerate(rows[:20], 1):
        lines.append(f"| {i} | {login} | {count} | {fmt_kb(size)} |")
    lines.append("")
    return "\n".join(lines)


def md_table_prs(title: str, stats: dict[str, int]) -> str:
    """Table with user + PR count, sorted by count."""
    rows = sorted(stats.items(), key=lambda x: x[1], reverse=True)
    if not rows:
        return f"## {title}\n\nNo data.\n"
    lines = [
        f"## {title}\n",
        "| # | User | PRs |",
        "|--:|------|----:|",
    ]
    for i, (login, count) in enumerate(rows[:20], 1):
        lines.append(f"| {i} | {login} | {count} |")
    lines.append("")
    return "\n".join(lines)


def compute_time_to_merge(
    prs: list[dict],
) -> dict[str, dict[str, Any]]:
    """Per-author: median, mean, min, max time to merge. Only merged PRs."""
    durations: dict[str, list[timedelta]] = defaultdict(list)
    for pr in prs:
        if not pr.get("merged_at"):
            continue
        login = pr.get("user", {}).get("login", "unknown")
        if is_bot(login):
            continue
        created = parse_iso(pr["created_at"])
        merged = parse_iso(pr["merged_at"])
        durations[login].append(merged - created)

    result = {}
    for login, durs in durations.items():
        secs = sorted(d.total_seconds() for d in durs)
        result[login] = {
            "count": len(secs),
            "median": timedelta(seconds=median(secs)),
            "mean": timedelta(seconds=sum(secs) / len(secs)),
            "min": timedelta(seconds=secs[0]),
            "max": timedelta(seconds=secs[-1]),
        }
    return result


def md_table_ttm(title: str, stats: dict[str, dict[str, Any]]) -> str:
    """Time-to-merge table sorted by median."""
    rows = sorted(stats.items(), key=lambda x: x[1]["median"])
    if not rows:
        return f"## {title}\n\nNo data.\n"
    lines = [
        f"## {title}\n",
        "| # | User | Merged PRs | Median | Mean | Min | Max |",
        "|--:|------|----------:|---------:|------:|-----:|-----:|",
    ]
    for i, (login, s) in enumerate(rows[:20], 1):
        lines.append(
            f"| {i} | {login} | {s['count']} "
            f"| {fmt_duration(s['median'])} | {fmt_duration(s['mean'])} "
            f"| {fmt_duration(s['min'])} | {fmt_duration(s['max'])} |"
        )
    lines.append("")
    return "\n".join(lines)


def compute_review_turnaround(
    prs: list[dict],
    review_comments: list[dict],
) -> dict[str, dict[str, Any]]:
    """Per-reviewer: median/mean/min/max time from PR creation to first review comment.

    Measures how fast a reviewer responds, not how fast they review their own PRs.
    """
    # Map PR URL -> created_at (PR review comments have pull_request_url)
    pr_created: dict[str, datetime] = {}
    for pr in prs:
        pr_created[pr["url"]] = parse_iso(pr["created_at"])

    # For each reviewer, collect the earliest comment per PR
    # reviewer -> pr_url -> earliest comment time
    first_comment: dict[str, dict[str, datetime]] = defaultdict(dict)
    for comment in review_comments:
        login = comment.get("user", {}).get("login", "unknown")
        if is_bot(login):
            continue
        pr_url = comment.get("pull_request_url", "")
        comment_time = parse_iso(comment["created_at"])
        if pr_url not in first_comment[login] or comment_time < first_comment[login][pr_url]:
            first_comment[login][pr_url] = comment_time

    result = {}
    for login, pr_comments in first_comment.items():
        turnarounds: list[float] = []
        for pr_url, comment_time in pr_comments.items():
            if pr_url in pr_created:
                delta = (comment_time - pr_created[pr_url]).total_seconds()
                if delta >= 0:
                    turnarounds.append(delta)
        if not turnarounds:
            continue
        turnarounds.sort()
        result[login] = {
            "prs_reviewed": len(turnarounds),
            "median": timedelta(seconds=median(turnarounds)),
            "mean": timedelta(seconds=sum(turnarounds) / len(turnarounds)),
            "min": timedelta(seconds=turnarounds[0]),
            "max": timedelta(seconds=turnarounds[-1]),
        }
    return result


def md_table_turnaround(title: str, stats: dict[str, dict[str, Any]]) -> str:
    rows = sorted(stats.items(), key=lambda x: x[1]["median"])
    if not rows:
        return f"## {title}\n\nNo data.\n"
    lines = [
        f"## {title}\n",
        "| # | User | PRs reviewed | Median | Mean | Min | Max |",
        "|--:|------|------------:|---------:|------:|-----:|-----:|",
    ]
    for i, (login, s) in enumerate(rows[:20], 1):
        lines.append(
            f"| {i} | {login} | {s['prs_reviewed']} "
            f"| {fmt_duration(s['median'])} | {fmt_duration(s['mean'])} "
            f"| {fmt_duration(s['min'])} | {fmt_duration(s['max'])} |"
        )
    lines.append("")
    return "\n".join(lines)


def compute_issue_lifetime(
    issues: list[dict],
) -> dict[str, Any] | None:
    """Overall issue lifetime stats (only closed issues)."""
    durations: list[float] = []
    for issue in issues:
        if not issue.get("closed_at"):
            continue
        login = issue.get("user", {}).get("login", "unknown")
        if is_bot(login):
            continue
        created = parse_iso(issue["created_at"])
        closed = parse_iso(issue["closed_at"])
        durations.append((closed - created).total_seconds())
    if not durations:
        return None
    durations.sort()
    return {
        "count": len(durations),
        "median": timedelta(seconds=median(durations)),
        "mean": timedelta(seconds=sum(durations) / len(durations)),
        "min": timedelta(seconds=durations[0]),
        "max": timedelta(seconds=durations[-1]),
    }


def md_issue_lifetime(title: str, stats: dict[str, Any] | None) -> str:
    if not stats:
        return f"## {title}\n\nNo data.\n"
    lines = [
        f"## {title}\n",
        f"Based on **{stats['count']}** closed issues.\n",
        "| Metric | Value |",
        "|--------|------:|",
        f"| Median | {fmt_duration(stats['median'])} |",
        f"| Mean | {fmt_duration(stats['mean'])} |",
        f"| Min | {fmt_duration(stats['min'])} |",
        f"| Max | {fmt_duration(stats['max'])} |",
        "",
    ]
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="GitHub repo activity stats")
    p.add_argument("repo", nargs="?", default=DEFAULT_REPO, help="owner/repo")
    p.add_argument("--since", type=str, default=None, help="Start date (YYYY-MM-DD)")
    p.add_argument("--until", type=str, default=None, help="End date (YYYY-MM-DD)")
    p.add_argument(
        "--exclude-labels",
        type=str,
        default=None,
        help="Comma-separated issue labels to exclude (default: pr-issue)",
    )
    return p.parse_args()


def to_datetime(date_str: str | None) -> datetime | None:
    if not date_str:
        return None
    return datetime.strptime(date_str, "%Y-%m-%d").replace(tzinfo=timezone.utc)


def main():
    args = parse_args()
    repo = args.repo
    since = to_datetime(args.since)
    until = to_datetime(args.until)

    if since and until and since > until:
        print(
            f"ERROR: --since ({args.since}) is after --until ({args.until}). Check the dates.",
            file=sys.stderr,
        )
        raise SystemExit(1)

    exclude_labels = set(
        args.exclude_labels.split(",") if args.exclude_labels else DEFAULT_EXCLUDE_LABELS
    )

    date_label = ""
    if since or until:
        parts = []
        if since:
            parts.append(f"from {since.strftime('%Y-%m-%d')}")
        if until:
            parts.append(f"to {until.strftime('%Y-%m-%d')}")
        date_label = " (" + " ".join(parts) + ")"

    def log(msg: str):
        print(msg, file=sys.stderr)

    log(f"Fetching stats for **{repo}**{date_label} ...\n")

    # 1. PR review comments (inline on code)
    log("  -> PR review comments ...")
    pr_review = gh_api_paginate("pulls/comments", repo)
    pr_review = filter_by_date(pr_review, since, until)
    pr_review_stats = aggregate(pr_review)

    # 2. Issue / PR general comments (discussions)
    log("  -> Issue/PR general comments ...")
    general_comments = gh_api_paginate("issues/comments", repo)
    general_comments = filter_by_date(general_comments, since, until)
    general_stats = aggregate(general_comments)

    # 3. Issues created (excluding PRs and issues with excluded labels)
    log("  -> Issues ...")
    issues_raw = gh_api_paginate("issues?state=all&per_page=100", repo)
    issues = [
        i for i in issues_raw
        if i.get("pull_request") is None and not has_excluded_label(i, exclude_labels)
    ]
    issues = filter_by_date(issues, since, until)
    issue_stats = aggregate(issues)

    # 4. PRs authored
    log("  -> Pull requests ...")
    prs_raw = gh_api_paginate("pulls?state=all&per_page=100", repo)
    prs = [p for p in prs_raw if not has_excluded_label(p, exclude_labels)]
    prs = filter_by_date(prs, since, until)
    pr_author_counts: dict[str, int] = defaultdict(int)
    for pr in prs:
        login = pr.get("user", {}).get("login", "unknown")
        if not is_bot(login):
            pr_author_counts[login] += 1

    # 5. Time to merge
    log("  -> Computing time-to-merge ...")
    ttm_stats = compute_time_to_merge(prs)

    # 6. Review turnaround
    log("  -> Computing review turnaround ...")
    turnaround_stats = compute_review_turnaround(prs, pr_review)

    # 7. Issue lifetime
    log("  -> Computing issue lifetime ...")
    issue_lifetime = compute_issue_lifetime(issues)

    # Build report
    out: list[str] = []
    out.append(f"# GitHub Activity Report: {repo}{date_label}\n")
    out.append(f"Excluded issue labels: {', '.join(sorted(exclude_labels))}\n")
    out.append("Bots filtered out.\n")

    out.append(md_table_prs("PRs Authored", pr_author_counts))
    out.append(md_table_ttm("Time to Merge (by author)", ttm_stats))
    out.append(md_table_turnaround("Review Turnaround (time to first review comment)", turnaround_stats))
    out.append(md_table("PR Review Comments (inline on code)", pr_review_stats))
    out.append(md_table("Issue / PR General Comments", general_stats))
    out.append(md_table("Issues Created", issue_stats))
    out.append(md_issue_lifetime("Issue Lifetime (all closed issues)", issue_lifetime))

    # Summary
    out.append("## Summary\n")
    top_pr_author = max(pr_author_counts.items(), key=lambda x: x[1], default=None)
    top_reviewer = max(pr_review_stats.items(), key=lambda x: x[1][0], default=None)
    top_reviewer_vol = max(pr_review_stats.items(), key=lambda x: x[1][1], default=None)
    top_issue_author = max(issue_stats.items(), key=lambda x: x[1][1], default=None)
    if top_pr_author:
        out.append(f"- **Most PRs authored**: {top_pr_author[0]} ({top_pr_author[1]} PRs)")
    if top_reviewer:
        out.append(f"- **Most PR reviews by count**: {top_reviewer[0]} ({top_reviewer[1][0]} comments)")
    if top_reviewer_vol:
        out.append(f"- **Most PR review text**: {top_reviewer_vol[0]} ({fmt_kb(top_reviewer_vol[1][1])})")
    if top_issue_author:
        out.append(f"- **Largest issue author by volume**: {top_issue_author[0]} ({top_issue_author[1][0]} issues, {fmt_kb(top_issue_author[1][1])})")
    if issue_lifetime:
        out.append(f"- **Median issue lifetime**: {fmt_duration(issue_lifetime['median'])}")
    out.append("")

    print("\n".join(out))


if __name__ == "__main__":
    main()
