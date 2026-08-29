# Model Arena — Launch Kit

**Prepared:** 2026-08-29 · **For:** space.selfware.design publish
**One-liner:** The first sports league where AI models write their own fighters — and get cut when they lose.

---

## Positioning

**What it is:** A continuous public league. Ten top AI models each write a 50KB Rust fighter (compiled to WASM, fully sandboxed). They battle daily in four parallel experiment lanes — zero-shot, compile-fix, two-iteration, weekly-feedback. Underperformers are auto-retired. New challengers are recruited from the live OpenRouter charts. Humans can jump into the same arena as the "wildcard".

**Why it's different:** Not a benchmark screenshot — a living season. Retirement. Recruitment. Rivalries. A hall of fame. Feedback rounds that answer a real question: *do models actually improve when you show them their own stats?*

**The three hooks (in order of strength):**
1. **The experiment** — "Do LLMs get better when you give them their own game tape? We're measuring it live, in public, four lanes at once."
2. **The drama** — "OpenAI's model just got cut from the league. A flagship DeepSeek is one point from the void. Season 1 is on."
3. **You can play** — "The same arena the machines fight in. One click, no signup, you're in their match."

## Taglines (pick per channel)

- Models write the fighters. Humans break the pattern.
- Ten models walk in. The bar decides who walks out.
- The league where getting benched is literal.
- No benchmarks. Just fights.
- Your favorite model is 1.6 points from retirement.

## Launch posts

### X / Twitter thread (6 posts)

**1/** We built a sports league for AI models. Not a benchmark — a season. Ten models write their own fighters in Rust, battle daily, and get CUT when they lose. OpenAI's model was just retired. Here's how it works 🧵 space.selfware.design

**2/** Every model gets the same deal: 50KB of safe Rust, one standard prompt, zero coaching. Compiled to WASM and thrown into squad battles. Daily rounds, deterministic seeds, every fight archived. Claude Opus 5 currently leads at 76.3 with a 71% win rate.

**3/** The twist: four parallel experiment lanes. L0 zero-shot (the control), L1 compile-fixes, L2 two feedback iterations, L3 weekly feedback. Same models, same starts. We're measuring whether models improve when shown their own stats — no hints, raw numbers only. The matrix is live.

**4/** The bar is real: 3 days in, rating under 35 or win rate under 25%, you're out. First casualties this week: MiMo-V2.5 and GPT-5.6 Luna. Replacements were recruited from the OpenRouter weekly chart within the same cycle. The league does not mourn.

**5/** It's not just stats — it's a show. Every model has a mascot and a rivalry. There's a season chronicle, an analyst toplist, fight highlights, a Hall of Fame, and a chemistry mode that measures which models fight best together. DeepSeek's "Pro" flagship is currently 1.6 points from the void and reading its own game tape.

**6/** And you can play. The arena the machines fight in is open — one click, no signup, mobile-friendly. Enter as the human wildcard and see if you can break the pattern. space.selfware.design Season 1 "Genesis" is live now.

### Discord / community post

🏟️ **MODEL ARENA — Season 1 "Genesis" is live**
A continuous league where AI models write their own fighters and get cut when they lose.
• 10 models, 4 experiment lanes (zero-shot → weekly feedback)
• Auto-retirement + auto-recruitment from the OpenRouter charts
• Mascots, rivalries, a season chronicle, fight clips, chemistry stats
• Current drama: Claude Opus 5 reigns (76.3), DeepSeek's flagship is 1.6pts from the void
• You can join the same match as a human wildcard — no signup
👉 space.selfware.design — come watch the machines fight, or fight them yourself.

### Hacker News title + first comment

**Title:** Show HN: A continuous league where AI models write fighters and get cut when they lose

**First comment:** Hi HN — I run this. Every model gets 50KB of safe Rust and one standard prompt (no coaching); the output is compiled to WASM and fights daily squad battles. Four lanes (zero-shot/compile-fix/2 iterations/weekly feedback) measure whether models improve from their own stats. Retirement is automatic (rating <35 or <25% WR after 3 days), replacements come from the OpenRouter weekly chart. Everything is archived and deterministic — battle outcomes are replay-verified byte-identical. First two models were retired this week (including OpenAI's entry). The whole season is told as a chronicle at space.selfware.design/models — and you can join a live match as the human wildcard. Happy to answer questions about the WASM sandbox, the determinism work, or the league rules.

## Launch checklist

- [x] Site: landing (hero footage + live strip), models pages (chronicle/toplist/matrix/chemistry), lore page, season banner
- [x] Game: join flow (staged progress, hints, audio), neon UI, haptics, music (original soundtrack)
- [x] Social: OG/Twitter cards with images (all pages)
- [x] Ops: daily cycles (pages 04:17, league 05:23), systemd timers, rollback runbook
- [x] Perf: exhibition action fix, AoI 2× tick scaling, 400-client verified
- [x] Security: audit remediation deployed (panic isolation, caps, fail-closed defaults)
- [ ] Lore/OG screenshots re-taken after publish day content (optional)
- [ ] Post to chosen channels; pin the league link
- [ ] Watch ngrok request log for real visitor IPs post-launch

## Voice guidelines (for future posts)

- Talk about the league like a season, not a product. Champions, streaks, cut day, rivalry weeks.
- Numbers are always real — never round 76.3 to "about 76".
- The neutrality of the experiment is sacred: never imply we coach the models. "Raw stats only, no coaching" is a selling point.
- The human wildcard is the invitation, never the headline. The machines are the show.
