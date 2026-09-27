# 0007: Decision models for System 1 and System 2

- **Question:** should `s2w` plan on decision models like TypeSafe's Jev, and which of the
  models and services that appeared around it (open weights, encoders, local runtimes, hosted
  judges, entity matchers, calibration wrappers) are fast enough for a live stream, and fit which
  System 1 router rung or System 2 role? Dave, 2026-09-27: *"should we have sagan research
  emerging decision models like jev? i know there are some emerging and perhaps they have
  different use cases or performance characteristics?"* Same day: *"key for us if they are fast
  enough"*; and *"is the routing more than latency possibly? like one is more suited to
  classification, another is more suited to some other use case?"*
- **Date:** 2026-09-27 · **Researcher:** sagan (with three research subagents) · **Status:** done
- **Feeds:** the System 1 engine trait (`Engine::evaluate(event) → Propose | Abstain`, #51); the
  README's "System 1 router" and "System 1, decision models" rows (both *later*); gate 3's H+S2
  arm (contract B1) for the entity-match and type-check roles in System 2; gate 4's predictor set
  (contract A5, A6, A9.3); the scale envelope (decision 0004: 1,000 events/s on a 4-core laptop).

Conventions as in the index: every claim cites the page used; **UNVERIFIED** means seen only in
a search snippet, a summary, or a third-party write-up, not a fetched primary page. Facts and my
judgments are kept apart; judgments are labelled **Judgment**. This is a snapshot of a category
that is twelve days old: Jev launched 2026-09-15; the youngest model here shipped 2026-09-26.

---

## Verdict

1. **Only one engine in the category meets a per-event budget at 1,000 events/s. Everything
   else is a sampled, batched or asynchronous rung.** 1,000 events/s on one core is 1 ms per
   event; on the four-core envelope, 4 ms. Blink-tiny ([sqliteai](https://github.com/sqliteai/blink))
   decides in **54 µs** on a cached state and 291–406 µs cold, on one Apple M5 Pro core, from a
   zero-allocation C99 runtime that also builds to a 66 KB WASM module — and by its own README
   "recognises the *form* of a decision; it does not read text". The next tier, the 144M–421M
   encoder models (Verdict, Julia-1, Laya, GLiNER2.5-Decide, von), is **20–45 ms on a T4/V100
   GPU and 90–460 ms on a laptop CPU**: 2–11 decisions/s per core, so a 0.2–1% sample of the
   stream, or a GPU sidecar at ~100–300 decisions/s batched. Hosted Jev is 70–500 ms end to
   end with a 1,200 request/min limit that "can change without notice"
   ([docs/models](https://docs.typesafe.ai/models.md)): 20 events/s unbatched. Table §2 leads
   with these numbers.
2. **"Decision model" is now a wire format, not a vendor.** Jev
   ([typesafe.ai](https://typesafe.ai/blog/introducing-system-one-models-and-jev), 2026-09-15)
   defined `POST /v1/systemone` (state + typed questions → per-option probabilities). In twelve
   days at least a dozen open-weight models adopted that contract (§2), two gateways serve it
   ([Vercel](https://vercel.com/blog/ai-gateway-jev-model-launch),
   [LangSmith](https://docs.langchain.com/langsmith/llm-gateway-decision-models)), Cloudflare
   hosts Jev itself ([Workers AI](https://developers.cloudflare.com/ai/models/typesafe/jev/)),
   and an independent benchmark scores engines through it
   ([Decision Index](https://github.com/apolinario/decision-index)). **Judgment:** `s2w` should
   target the wire format behind the `Engine` trait, never one model. One adapter reaches Jev,
   Kev, Laya, Julia-1, Rune, CLM, von, Verdict and whatever ships next week; that is the "two
   real implementations per seam" rule the README already imposes, and it is what makes Dave's
   "explosion of these system1 models" an asset rather than churn (§6).
3. **Jev leads on reading comprehension and is a calibration black box.** It tops Decision
   Index 0.2.1 (57.89 vs Rune 57.44 — **UNVERIFIED** rows via search summary; Rune's own card
   says it "ranks second" ([HF](https://huggingface.co/surogate/rune-26b-a4b-GGUF))) and
   independent runs confirm its latency and price
   ([Every: 777 judgments in <0.7 s](https://every.to/vibe-check/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds)).
   TypeSafe publishes no calibration curve, Brier, ECE, size or architecture
   ([pearpages](https://pearpages.com/blog/2026/09/16/jev-sorted-what-typesafes-system-one-model-actually-is-and-what-is-still-just-a-claim);
   CEO: *"I probably shouldn't talk too much about the insides of ML"*,
   [Latent Space](https://www.latent.space/p/jev)). Its accuracy figure is agreement with GPT-6
   Astra and Fable 5.1, not human labels (67.8% mean, 61.8–76.0% across four workflows,
   [DataCamp](https://www.datacamp.com/blog/system-one-models-jev),
   [dev.to critique](https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k)).
   Third parties that measured it got ECE 0.063–0.281 depending on dataset
   (§2b). Gate 4's calibration criterion (A9.3) has to be measured per engine; nothing published
   lets us assume it for Jev or anyone else.
4. **No decision model abstains; every open encoder ships over-confident.** Diogo Almeida calls
   a refusal "a type error" ([Latent Space](https://www.latent.space/p/jev), 00:12:18); the
   whole class returns a full distribution and leaves "don't act" to code. Three independent
   harnesses agree that the open encoders ship with raw ECE 0.16–0.47 and drop to 0.03–0.08
   after a temperature is fitted on a few hundred labels (§2b); only Kev and Verdict ship a
   fitted temperature. `s2w`'s `Abstain{reason}` is therefore derived in the adapter (§7a), and
   a per-engine, per-stream temperature is part of the adapter's state, not the model's.
   ⚠️ Vendor `confidence` fields use different formulas (Jev `(n·p_max − 1)/(n − 1)`; Laya
   `1 − normalised entropy`, [README](https://github.com/NandhaKishorM/laya)); gate on the
   probability of the reported answer, which every engine returns.
5. **For entity matching a decision model is a judge over candidate pairs, not a matcher.**
   Every worked example (TypeSafe's
   [entity_alignment cookbook](https://docs.typesafe.ai/cookbooks/entity_alignment.md),
   [Southbridge's campaign-finance pipeline](https://www.southbridge.ai/blog/jev-entity-resolution))
   has a cheap first pass produce pairs and a three-level Score judge each pair. That is where H
   already ends (research 0002 §4–§5): decision models slot in at the pairs H marks ambiguous,
   on the plain and private streams; on the obfuscated stream they have nothing to read and
   abstain by construction. The calibrated bulk of pair scoring belongs to a Fellegi–Sunter
   matcher (Splink-class, §4), which is the number a decision model must beat on the residue.
6. **Routing is more than latency, and nobody has built the router `s2w` needs.** The
   candidates differ on judgment kind, language, calibration, abstain shape, cost and measured
   accuracy per stream (§8). The prior art — cascades, learned routers, bandits — routes
   *prompts to LLMs*; none routes *typed judgments to decision engines* on persisted verdicts
   with late outcomes. The research question in §8e is `s2w`'s to state.
7. **Nobody has measured any of these on an event stream.** Every published number is
   documents, tickets, transcripts or benchmark rows; the one bag-of-words baseline that was run
   came within 11 points of Jev (§2b). The README's "nobody has measured Jev's latency, cost or
   accuracy on these questions" now holds for the whole category; §10 says how the spike ends
   that.

---

## 1. What a decision model is, mechanically

One contract, stated once ([TypeSafe API](https://docs.typesafe.ai/api.md)): `POST /v1/systemone`
with `state` (string, JSON object or array), `model`, and a map of named `questions`:

| Primitive | Answer | `s2w` reading |
|---|---|---|
| `noul` | P(yes) | one claim's probability; confidence in basis points is `round(p · 10^4)` |
| `choice` (≤255 options) | one option + full distribution + `confidence` | type label, field-role label, engine selection |
| `score` (ordered levels) | expected level + distribution + `confidence` | `same / related / different` for a pair; anomaly severity |

All questions in one request are evaluated against the state in parallel and cannot see each
other ([parallel_questions cookbook](https://docs.typesafe.ai/cookbooks/parallel_questions.md):
13 questions in one call 0.27 s vs 13 calls 2.71 s; run-to-run std dev of answers 0.0 either way).

**How the probability is produced** differs by family, and it decides calibration and speed:

- *Encoder + decision head* (Laya, Julia-1, Verdict, von, GLiNER2.5-Decide, dev-0.4b, Blink): a
  bidirectional encoder over state + question + options, a head per option, softmax. One pass,
  no decoding. Calibration is a post-hoc temperature per checkpoint (Laya: raw ECE 0.213 → 0.081
  after scaling, [HF](https://huggingface.co/convaiinnovations/laya)).
- *Decoder + LoRA + pointer head* (Kev, Decider, Intern-Decision, Reflex, JevK5, Eikos): a
  0.8B–35B instruction LLM with a head that points at option tokens; one pass, no generation.
  Kev-4B: T=2.41 fitted, OOD ECE 0.042, Brier 0.265 → 0.243
  ([HF card](https://huggingface.co/jaredpalmer/kev-4b/blob/main/README.md)).
- *Option-logit read from a stock LLM* (SemIf, [AnyJev](https://github.com/nokia-applied-research/AnyJev),
  TypeSafe's own [system-one-adapter](https://github.com/typesafe-ai/system-one-adapter-python)):
  prompt, read the next-token distribution over option markers, renormalise. No training. This
  is the only way a raw-LLM baseline (contract B3) produces probabilities at all, and
  "The Parser Already Knows" ([arXiv 2608.10137](https://arxiv.org/pdf/2608.10137)) shows the
  grammar mask biases those probabilities unless corrected.
- *Contrastive state/action embeddings* (CLM-8B): state and each option embedded separately, a
  similarity score; option embeddings cached across calls
  ([README](https://github.com/Contrastive-LM/CLM): 58.1 ms per request behind a Qwen3-8B
  pooling server).
- *MoE decoder with a decisions endpoint* (Surogate Rune 26B-A4B): Gemma 4 base, optional
  "thinking" when confidence < 0.7 (~5 s) ([HF](https://huggingface.co/surogate/rune-26b-a4b-GGUF)).
- *Jev*: undisclosed. HN commenters infer an encoder with scalar/ordinal heads
  ([HN, 1,984 points](https://news.ycombinator.com/item?id=49717558): quotemstr, bregmandiv);
  Verdict's README lists "~150M" with a dagger — **UNVERIFIED**, and I would not repeat it.

**Training for calibration.** TypeSafe names its method RLCD, unpublished
([AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer.md)). Laya, von and
Verdict claim to reproduce it with proper scoring rules and publish their recipes; whether that
is what TypeSafe does is unknown.

---

## 2. Candidates: decision models (the Jev class), latency first

**Feasibility column.** "Per-event" = under 1 ms per decision on one CPU core, so the engine can
sit on the event path at 1,000 events/s (decision 0004). "Sampled" = 20–460 ms on CPU, so it can
judge 0.2–5% of events synchronously or all of them on a GPU sidecar, batched. "Async" = hosted or
> 50 ms, off the path. Latencies are as published by each project on the hardware they name;
p99 is given only where published; none is measured by us (§10 is where that changes).

| Model | Latency per decision (published) | Keeps up at 1,000 ev/s? | Size · family | Local / API | Calibration (published) | Abstain | Pin | Licence | Langs | Org · date · maturity |
|---|---|---|---|---|---|---|---|---|---|---|
| **Blink-tiny / -small** | **54 µs** cached state, 291–406 µs cold (tiny); 4.29 ms/512 B (small); 824 µs–1.45 ms with Accelerate; one M5 Pro core; WASM ≈ 0.8 ms | **per-event** (tiny); small is ~230/s per core, ~930/s on 4 cores | 457 KB weights (tiny); 7.9M (small); encoder head, form-only | in-process C99, zero-alloc; 66 KB WASM; Python | ECE 0.003–0.056 on its own suites | gated | file hash | not in fetched text (**UNVERIFIED**) | no | sqliteai · 2026-09 · WANLI ≈ chance by design |
| **openJev-verdict-2.0** | 20–25 ms GPU; < 35 ms WebGPU in Chrome; CPU unknown | sampled (~40/s on GPU) | 149.6M ModernBERT-base | local; browser | ECE 0.0144 on a second confidence head; Brier 0.0636 (2,000 rows) | gated; correctness head | Git LFS | Apache-2.0 | EN | Heman10x · 2026-09 · trained on states < 71 tokens, ctx cut to 512 |
| **Julia-1** | 89–294 ms per decision on an Intel i5-1235U; 28–51 decisions/s on Apple M4; 5/s on a Samsung SM-X510 tablet | sampled (0.3–1% of the stream per core on a laptop; ~4% on an M4) | 144.3M mmBERT-small; 550 MiB; 1,024-token ctx | local, CPU-first; API planned at $0.025/MTok | none published; "can abstain … abstentions count as errors" | **yes, native** (the only one) | HF | Apache-2.0 | multilingual base; MASSIVE 71.5% over 52 locales | Supersonic Labs (Brazil) · 2026-09-26 · trained for US$104.08; [page](https://supersoniclabs.ia.br/julia-1/), [@omarsar0](https://x.com/omarsar0/status/2104235323970523543) |
| **Laya** | 33–39.5 ms one question, 7.2 ms/q for 10 batched (T4); 193–464 ms CPU; script router < 0.5 ms | sampled on CPU (~2–5/s per core); ~140/s batched on a T4 | 421M ModernBERT-large (EN, 512 ctx); 322M mmBERT-base (100+ langs, 8k ctx) | local: PyTorch, ONNX INT8, MLX, CoreML, laya.cpp, WebGPU, Node | raw ECE 0.213 → 0.081 scaled; 3rd-party raw 0.13–0.61 | opt-in `min_confidence` → `None` | `LAYA_REVISION` + SHA-256 per checkpoint | Apache-2.0 | yes; EN checkpoint "0.000 accuracy at 0.952 confidence" on Khmer | Convai · 2026-09-18 · largest runtime ecosystem ([README](https://github.com/NandhaKishorM/laya)) |
| **GLiNER2.5-Decide** | p50 38.3 ms V100, 43.6 ms T4, 47.3 ms A100 (64 tok); **167.3 ms on a 48-vCPU Xeon**; ~300 ms Ryzen 9800X3D, ~25 ms RTX 5080 (3rd party); 37.8 ms iPhone 18 Pro | sampled | 340M DeBERTa-v3-large encoder + joint decoder; 1B Ettin sibling scores lower | local; CoreML ports | none from Fastino; 3rd party raw ECE 0.160 → 0.028 at T=2.2 | "none" option in multi-label mode | HF | Apache-2.0 | EN; `-multi-Decide` 287M | Fastino · 2026-09-26 · leads its own 17-dataset suite ([blog](https://fastino.ai/blog/gliner-2-5-decide-open-weight-decision-model), [HF](https://huggingface.co/fastino/GLiNER2.5-Decide)) |
| **von 1.2** | ~18 ms GPU ("sub-15 ms" claimed, hardware unnamed) | sampled | 395M ModernBERT-large | local, Python/TS | "near-ideal ECE" (no number); T≈1.04–1.17 | gated | tags | Apache-2.0 | unknown | wfzyx · 2026-09 · 726 stars ([repo](https://github.com/wfzyx/von)) |
| **Intern-Decision-4B** | mean 44.2 ms, **p95 44.6 ms** on an RTX 4090 | sampled (~22/s) | Qwen3.5-4B + head; text + image | local | ECE 0.065; Brier 0.347 | unknown | HF | Apache-2.0 | Qwen base | InternLM · 2026-09 · 90.0% avg over 7 suites ([HF](https://huggingface.co/internlm/Intern-Decision-4B)) |
| **Kev 4B** (0.6B–27B) | 18 ms (4B, H100) – 46 ms (27B, B200) per request; ~300 ms for 5 q on a 32 GB Mac; ~2 s on Mac for 9B (**UNVERIFIED** Mac figures) | sampled on GPU; not viable on CPU | Qwen3.5-4B-Base + LoRA r=16 + pointer head; ~9 GB bf16 | local, or OpenRouter at $0.042/MTok | ships fitted T (2.41); OOD ECE 0.042, Brier 0.243; coverage 62% at ≤ 5% error | gated; coverage curve published | HF revision | Apache-2.0 | EN (Qwen base untested) | Jared Palmer · 2026-09-25 · training code public ([HF](https://huggingface.co/jaredpalmer/kev-4b/blob/main/README.md), [repo](https://github.com/jaredpalmer/kev)) |
| **Decider 35B-A3B** (0.8B/2B) | 41–49 ms on GH200 with ticket contexts | sampled, GPU only | Qwen3.5 fine-tunes, slot readout; MoE 3B active | local | "best calibrated" on the community index (metric unstated) | unknown | HF | Apache-2.0 | unknown | Mapika · 2026-09 ([collection](https://huggingface.co/collections/Mapika/decider)) |
| **CLM-8B** | 58.1 ms per request (38 tokens); "up to 9× lower latency than Jev" (vendor) | async | Qwen3-8B pooling encoder + 75 MB contrastive head | local, vLLM | none published | gated | HF | Apache-2.0 | unknown | Contrastive-LM · 2026-09 · option embeddings cached ([README](https://github.com/Contrastive-LM/CLM)) |
| **Surogate Rune 26B-A4B v3** | 90–180 ms median RTX PRO 6000 (bf16, 51.6 GB); ~5 s with thinking; 42 ms RTX 5090 (v2, **UNVERIFIED**) | async | Gemma 4 26B MoE, 8/128 experts active; text + images; 262k ctx | local; "no GGUF of v3 yet" | ECE 12.5% at T=1 → 2.2% at T=2; images 5.1% | opt-in thinking below 0.7 | HF | Apache-2.0 | yes | Invergent · 2026-09-26 · Decision Index #2 ([HF](https://huggingface.co/surogate/rune-26b-a4b-GGUF)) |
| **Jev 1.13** | 70–500 ms e2e (vendor); 3rd-party p50 236–349 ms, p95 483–593 ms; 1,200 req/min, 250k tok/s | async (20 ev/s unbatched at the rate limit) | undisclosed | API only, early access; also Cloudflare Workers AI, Vercel, LangSmith (BYOK) | none published; 3rd-party ECE 0.063–0.281 by dataset | no ("a type error") | `jev-1.13.0`; `jev-latest` moves; response reports id | proprietary; not trained on customer data; ZDR enterprise | EN best; CJK "not equally well" | TypeSafe · 2026-09-15 · 1,984-pt HN thread; 13% of Vercel paid teams in 24 h |
| **SemIf** (`semif-qwen3.5-4b`) | unknown | async | stock Qwen3.5-4B, option logits | local (3090) or LangSmith gateway | none | gated | gateway id | open source; repo README 404 (**UNVERIFIED** licence) | Qwen base | Theo Lee / LangChain · 2026-09-21 |
| **Tev1-4B-experimental** | unknown | async | Qwen3.5-4B SFT; emits one option letter (autoregressive) | Together API $0.042/MTok, or weights | README: logprobs "model preferences, not calibrated confidence" | no | API id | MIT code | unknown | Together · 2026-09-23 · $17 to train ([repo](https://github.com/togethercomputer/tev1)) |
| **AnyJev** | one prefill of the host LLM | depends on host | library: any open LLM → typed decisions; closed-form head from 100–300 labels | local | measured on your labels | no | n/a | Apache-2.0 | host | Nokia Applied Research · 2026-09-21 |
| **CUA-S1** | unknown | unknown | small, computer-use forms | local | unknown | unknown | HF | MIT code | n/a | Cua · 2026-09 · "engineering analogy … not a replacement for planning" |
| Not examined | Reflex, JevK5, AgentJev 0.6B, dev-0.4b, Eikos, OpenThai-SystemOne, Open-Jev 27B, OpenJev (DiffusionGemma), Lavoir, JPT-0.8B/9B, Bespoke Nimble, Valen, laya-vision | | | | | | | | | [awesome-decision-models](https://github.com/sfmqrb/awesome-decision-models), [awesome-jev-alternatives](https://github.com/mturac/awesome-jev-alternatives) |

Other row sources: Blink [README](https://github.com/sqliteai/blink), [RESULTS.md](https://github.com/sqliteai/blink/blob/main/docs/RESULTS.md);
Verdict [README](https://github.com/Heman10x-NGU/openJev-verdict-2.0); Jev [docs/models](https://docs.typesafe.ai/models.md),
[docs/api](https://docs.typesafe.ai/api.md), [jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md),
[Cloudflare](https://developers.cloudflare.com/ai/models/typesafe/jev/); Kev [OpenRouter](https://x.com/OpenRouter/status/2103560670205886744);
SemIf [LangSmith](https://docs.langchain.com/langsmith/llm-gateway-decision-models); third-party
calibration/latency harnesses: [elcronos](https://github.com/elcronos/jev-vs-open-decision-models),
[AbdelStark/jev-benchmarks](https://github.com/AbdelStark/jev-benchmarks), [decide-lab](https://github.com/turlockmike/decide-lab);
directory [systemonemodels.org](https://systemonemodels.org/).

**Judgment on the column that matters.** Read down "keeps up": one per-event engine, and it does
not read; a dozen sampled engines at 20–50 ms on a GPU we may not have; the rest async. So a
decision model on the event path is a *form* classifier over a cached state, and everything
that reads text is either a sample of the stream or a System 2 role. The CPU figures are the
ones that bind on the decision-0004 laptop: Julia-1's 89–294 ms and Laya's 193–464 ms are the
same order, and both are ~200× the per-event budget.

### 2a. What Jev's own docs say that matter for `s2w`

- **Price and limits:** $0.042 per million input tokens, output free; 250,000 tok/s and 1,200
  req/min, "adjusting dynamically … can change without notice"; 64k context, 32k for state plus
  the longest question ([docs/models](https://docs.typesafe.ai/models.md)). At ~300 tokens per
  event that is **$0.0126 per 1,000 events** and, unbatched, 20 events/s. Batching events into
  one state changes the question from per-event to per-batch.
- **Pinning:** `jev-1.13.0` is stable; `jev-latest` moves on release; the response's `model`
  field reports which id answered. Replay is by persisted verdict carrying that id, never by
  re-query.
- **Jaggedness** ([docs](https://docs.typesafe.ai/model-jaggedness/jev-1.13.md)): accuracy
  "falls as the state grows with content unrelated to the decision"; counting unreliable; "not
  a calculator"; dates read as text; double negatives and multi-hop hurt; `P(noul)` and
  `1 − P(not noul)` "may not be directly comparable" (0.72 + 0.47 = 1.19 in their own example);
  injected instructions in state can steer answers; semantic beats numeric representation.
  **Judgment:** an `s2w` event is the adversarial case — numbers, ids, timestamps, and under
  obfuscation, hashes. This is a model built for prose *about* state, not for state.
- **Entity alignment cookbook** ([docs](https://docs.typesafe.ai/cookbooks/entity_alignment.md),
  `jev-1.12`, 2026-08-11): 450 Magellan Beer pairs, one three-level Score plus three field Nouls
  per request; 360 unlinked / 50 to a curator / 40 merged. No precision/recall against
  `known_same_as` is printed. Scores cluster near 0.25, not on integers; 47 pairs within 0.1 of
  the lower cut, 9 within 0.1 of the upper.
- **Southbridge, the closest thing to our use** ([2026-09-20](https://www.southbridge.ai/blog/jev-entity-resolution)):
  donor/committee matching over Ohio campaign-finance rows, 200 families / 903 rows in 52
  batches. Jev alone 199/200 families with 4 declines; Jev + 5 GPT-5.6 Luna reviews 199/200;
  Fable 200/200. $0.034 vs an extrapolated $8.16M per TB for Fable. Failure modes named: a 0.53
  near-tie accepted as an answer; "the pipeline refuses to compose a merge across an uncertainty
  fence." **Judgment:** the same three-way Score, the same escalate-the-middle pattern, and the
  same failure (a near-tie is not a decision) that `s2w` will meet.
- **Independent runs:** Every, 37 documents × 21 questions = 777 judgments in under 0.7 s for
  ~$0.0025 ([Every](https://every.to/vibe-check/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds));
  Vercel replaced GPT-5.6 Luna in a command-safety check, "5–18× faster" with higher accuracy
  (**UNVERIFIED**, via [dev.ua](https://dev.ua/en/news/jev-1789979579); the
  [Vercel post](https://vercel.com/blog/ai-gateway-jev-model-launch) carries adoption numbers only);
  eesel, 93% triage on 100 tickets, no baseline ([eesel](https://www.eesel.ai/blog/typesafe-jev-review)).

### 2b. Benchmarks that exist for the class

| Benchmark | What it measures | Who leads | Caveat |
|---|---|---|---|
| [Decision Index 0.2.1](https://github.com/apolinario/decision-index) (2026-09-27) | 38 public benchmarks through `/v1/systemone`, chance-corrected, unanswered = wrong; reproducible kit | Jev 57.89, Rune 57.44, Laya 16.4 (**UNVERIFIED** rows; kit verified; board page 401) | knowledge-heavy (MMLU-Pro, GPQA, HLE); measures reading, not stream state |
| Fastino "Fast Decisions" | 17 datasets, 5,100 rows, exact match | GLiNER2.5-Decide 60.1%, JevK5 57.5, SemIf 56.4, Laya 46.6 | Fastino's own; Jev not in the table |
| [`LocalLLaMA/typed-decisions`](https://huggingface.co/datasets/LocalLLaMA/typed-decisions) | 1,600 synthetic rows, four ops domains | Verdict 77.1%, Laya 76.6%, Julia-1 73.15%, Jev 72.7%; **TF-IDF + logistic 66.1% at ~8 ms, ECE 0.021** | synthetic labels, method unstated; a bag-of-words baseline is within 11 points of Jev |
| TypeSafe's four workflows | agreement with GPT-6 Astra + Fable 5.1 | Jev 67.8% ≈ GPT-5.6 Terra 67.9%; Sol 74.1%, Opus 5 73.1% | model-labelled; circular where the references err |
| Independent calibration harnesses | ECE raw vs after a fitted temperature | every open encoder raw 0.16–0.47 → 0.03–0.08 with ~100–300 labels; Jev 0.063–0.281 by dataset; PrismNLI raw 0.17–0.20 | [elcronos](https://github.com/elcronos/jev-vs-open-decision-models), [AbdelStark](https://github.com/AbdelStark/jev-benchmarks), [decide-lab](https://github.com/turlockmike/decide-lab) — small sets, one week old |

**Judgment:** none of these is a stream, and the one trivial baseline that was run came close.
Gate 3's shape (H first, then H+S2, then B3) is the right shape for this category too.

---

## 3. Candidates: classical small classifiers, zero-shot and judge models

The pre-Jev landscape, kept because it is where the published throughput still is. Subagent
survey; each row cites the page fetched.

| Candidate | Task shape | Latency (published) | Keeps up? | Local/API | Calibration | Abstain | Licence · date |
|---|---|---|---|---|---|---|---|
| **Encoder + logistic head** (Ettin 17M–1B, mmBERT 140M/307M, NeoBERT 250M, EuroBERT 210M–2.1B, EmbeddingGemma 308M) | any supervised label over a cached embedding | encoder pass tens of ms on CPU for 17M–150M; **the head itself is µs**; no page publishes ms | per-event only if the embedding is already computed for another reason (H's local embeddings) | local (`ort`, `fastembed`) | temperature on your validation set | your threshold | MIT / Apache / CC-BY · 2025 ([Ettin](https://huggingface.co/blog/ettin), [mmBERT](https://arxiv.org/html/2509.06888v1)) |
| **Opir-edge / -large** (Knowledgator) | safety multi-task + 996-category zero-shot | **p50 9.25 ms edge (Ettin-32m), 25.65 ms large, at 1,024 tokens**, GPU unspecified; 499 samples/s | sampled | local | unknown | threshold | CC-BY-4.0 paper · 2026-05 ([arXiv 2605.29659](https://arxiv.org/html/2605.29659)) |
| **GLiClass** modern-base 151M / large 399M | zero/few-shot multi-label, one pass | 137–190 ms CPU (per GLiNER2 paper) | sampled | local | unknown | per-label sigmoid threshold | Apache-2.0 · 2025-02 ([HF](https://huggingface.co/knowledgator/gliclass-modern-base-v2.0-init)) |
| **GLiNER2 / 2.5-multi** 205M / 287M | NER + classify + relations from one schema | 130–208 ms CPU by label count vs DeBERTa-NLI 1.7–16.9 s | sampled | local | none in paper | none | Apache-2.0 · 2025-07 ([arXiv 2507.18546](https://arxiv.org/html/2507.18546v1)) |
| **DeBERTa-v3 NLI zero-shot / PrismNLI-0.4B** | entailment as classification; cost × label count | PrismNLI p50 58 ms M1 Max; raw ECE 0.17–0.20 | sampled | local | no | threshold | MIT / CC-BY-4.0 |
| **SetFit** | few-shot on sentence-transformer + LR head | "5–15× faster than the ZS pipeline"; no ms | sampled | local | no by default | threshold | Apache-2.0 · 2022 |
| **TabPFN-2.5** (Prior Labs) | in-context tabular classifier; distills to MLP/trees | "orders of magnitude lower latency" after distillation; no ms | unknown | local | yes (Bayesian) | threshold | 2025-11 ([arXiv](https://arxiv.org/html/2511.08667v2)) — only if the event is a feature vector |
| **Luna-2** (Galileo) | single-token True/False judge, 3B/8B, LoRA per metric | ~150 ms A100 at 1,250 tok; 15 ms small requests on H100 | async | enterprise platform only | logprob-derived; no ECE | threshold | proprietary · 2026-02 ([arXiv 2602.18583](https://arxiv.org/html/2602.18583)) |
| **Skywork-Reward-V2 0.6B** | scalar reward / pairwise | unknown | async | local | no | no | Apache-2.0 · 2025-07 |
| **Selene 1 Mini 8B / GLIDER 3.8B / Flow-Judge 3.8B / Prometheus 2 7B** | generative rubric judges | unknown / ~1 s / unknown / unknown | async | local | no | no | mixed; GLIDER CC-BY-NC; Atla's site 404 (**UNVERIFIED** status) |
| **Qwen3Guard 0.6B (Stream) / ShieldGemma 2B / OpenAI omni-moderation** | safety classify; Qwen3Guard-Stream has token-level heads | unknown (3rd-party 0.5–1.5 s Gen path, **UNVERIFIED**) | async | local / local / API | logit prob (ShieldGemma) | Controversial tier (Qwen3Guard) | Apache / Gemma / free |
| Cohere Classify · Mistral Classifier Factory · Fastino TLM | "classifier as a product" | — | — | — | — | — | **Cohere retired 2025-09-15** ([deprecations](https://docs.cohere.com/docs/deprecations)); Mistral's page is under `/deprecated/` while pricing still lists a "Classifier API" ([docs](https://docs.mistral.ai/resources/deprecated/finetuning/classifier_factory)) |

Two facts from this section that survive into the design: the only sub-millisecond path anyone
can name is a linear head over an embedding the pipeline already computed; and the classifier-
as-a-product API tier that existed before Jev has largely closed, which is part of why the
category filled so fast (§6).

---

## 4. Candidates: local structured-output LLMs, and entity-resolution models

### 4a. Small LLM + constrained decoding, as a classification engine

Subagent survey. Two results shape the whole class:

- Constrained decoding fixes format, not judgment ([arXiv 2609.23742](https://arxiv.org/html/2609.23742v1),
  2026-09-20): Qwen3 0.6B/4B, Llama 3.2 1B/3B, Phi-4-mini under Outlines and XGrammar go from
  78.6–92.9% schema validity to 100%, while the semantic error class stays scale-dependent.
  XGrammar costs 4–8 ms compile and 1.6–3.7% throughput; Outlines 2–19.5 s compile per schema.
- Probabilities read under a grammar mask are biased toward valid tokens
  ([arXiv 2608.10137](https://arxiv.org/pdf/2608.10137)); a parser-informed correction helps.

| Piece | What it gives `s2w` | Latency (published) | Keeps up? | Logprobs | Abstain | Licence · maturity |
|---|---|---|---|---|---|---|
| `llama-cpp-2` 0.1.157 (2026-09-22) | Rust bindings; `LlamaSampler::grammar`, readable `LlamaTokenDataArray` before/after mask | = llama.cpp | async | yes | via a GBNF enum incl. `unknown` | MIT/Apache; tracks upstream, API churn ([lib.rs](https://lib.rs/crates/llama-cpp-2)) |
| `llguidance` 1.8.0 | mask computation, ~50 µs/token; JSON Schema, regex, Lark | — | — | n/a | via schema | MIT Rust crate; in llama.cpp, mistral.rs, vLLM ([repo](https://github.com/guidance-ai/llguidance)) |
| `mistral.rs` 0.8.x · `candle` 0.11 · `ort` rc.13 | Rust server / tensors / ONNX; `ort` is the right runtime for the **encoder** models of §2 and §3 | unknown | — | raw logits | DIY | MIT/Apache; `ort` ~19.8M downloads ([ort](https://github.com/pykeio/ort)) |
| Gemma 4 E2B (Apache-2.0, 2026-07-02) | strongest licence + speed in the Gemma line | i7-12700 Q4 ~15 tok/s; M3 Max 108–114 tok/s | async | yes | enum | [HF](https://huggingface.co/google/gemma-4-E2B-it) |
| Qwen3.5-0.8B (Apache-2.0, 2026-03) | tiny, 201 languages; the 2606.08051 paper's best latency/accuracy point for extraction | ~1.3 samples/s on a 10-field JSON extract (DGX Spark) | async | yes | enum | [HF GGUF](https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF) |
| Gemma 3 270M / 1B, Phi-4-mini, Llama 3.2 1B/3B, Granite 4.0 Nano, SmolLM3-3B, Ministral 3 3B | the rest of the sub-4B field | Pi 5: 22 / 10 tok/s (Gemma 270M / 1B); i7-12700: 12 tok/s (Phi-4-mini), 10 (Llama 3B); others unknown | async | yes | enum | Gemma ToU / MIT / Llama Community / Apache |

**Judgment:** no page publishes ms-per-call for a short classify on a laptop CPU; at 10–100
tok/s a 300-token prompt is 3–30 s of prefill on CPU, which puts this class in System 2 or
behind a GPU. It is the *B3 baseline's* mechanism, and a fine-tuned encoder via `ort` beats it
for any fixed decision on latency and calibration.

### 4b. Entity resolution and matching

| Candidate | Task | Latency / throughput | Keeps up? | Calibrated probability | Abstain | Licence · Rust | Notes |
|---|---|---|---|---|---|---|---|
| **Splink 4.0.17** (2026-09-03; v5 dev) | probabilistic linkage, pairwise scoring | 1M records/min batch; per-pair unknown; `compare_two_records` / `find_matches_to_new_records` for real time | sampled (per-pair, DuckDB) | **yes: Fellegi–Sunter posterior**, `Pr = 2^M/(1+2^M)`, m/u from EM; valid to the extent of conditional independence | two-threshold indeterminate band, yours to set | MIT · no Rust | the calibrated bulk scorer; [theory](https://moj-analytical-services.github.io/splink/topic_guides/theory/fellegi_sunter.html), [real-time demo](https://moj-analytical-services.github.io/splink/demos/examples/duckdb/real_time_record_linkage.html) |
| **GoldenMatch v3.22** (2026-03 → 2026-09-27) | zero-config F-S, incremental index, Python + TS/WASM + "Rust authoritative kernels" | 100M rows / 9.2 min on 5-node Ray (self-reported); F1 0.827 vs Splink 0.757 on its own set | unknown | claims calibrated | unknown | MIT · **Rust kernels** | six months old, one maintainer; the only F-S with Rust in it ([repo](https://github.com/benseverndev-oss/goldenmatch)) |
| **Ditto-style fine-tuned RoBERTa/DeBERTa pair classifier** | (a, b) → P(same) | encoder ms-scale via `ort`; exact unknown | sampled | sigmoid; ECE 0.004–0.055 on six EM sets, temperature cuts it up to 23.8% ([arXiv 2509.19557](https://arxiv.org/html/2509.19557v2)) | band on the scaled probability | model-dependent · `ort` | no canonical checkpoint; you fine-tune ([Ditto](https://arxiv.org/pdf/2004.00584)) |
| **LLM pair judge** (OpenSanctions Pairs, [arXiv 2603.11051](https://arxiv.org/html/2603.11051v2)) | 755,540 labelled pairs over 1M sanctions/PEP entities | 25–70 s per example (8B–14B) | no | logit-based UQ possible ([arXiv 2510.01251](https://arxiv.org/pdf/2510.01251)) | via logits | data CC-BY-NC | GPT-4o 98.95 F1; rule-based nomenklatura 91.3 — and "ER in Practice" argues for an explicit undetermined state before any transitive merge ([arXiv 2607.26298](https://arxiv.org/abs/2607.26298)) |
| **Decision model as pair judge** (Jev / Kev / Rune, §2) | three-level Score + field Nouls | 40 ms–500 ms per pair | sampled/async | see §2 | derived | see §2 | the cookbook and Southbridge shape |
| **Embeddings for candidates**: bge-m3 (MIT), nomic-embed-v2-moe (Apache), arctic-embed xs 22M–l 335M (Apache), jina-v4 (non-commercial), Voyage 4 (API) | ranking signal for blocking | arctic-xs "designed for strict latency budgets", no ms; Voyage ~52–122 ms/doc (**UNVERIFIED**) | sampled | no (cosine) | no | via `fastembed` 7.1.0 (Rust, [lib.rs](https://lib.rs/crates/fastembed)) | calibrate with a held-out isotonic/Platt fit |
| **MinHash / LSH in Rust**: `gaoya` 0.2.2 (2026-06), `probminhash` 0.1.12 | blocking | sub-ms | **per-event** | no | no | MIT | the only per-event piece in this table; H's containment tier already covers the exact-set case |
| Zingg 0.7.0 · dedupe 3.0.3 · Senzing v4 | Spark ML dedupe · LR + Gazetteer · commercial real-time lib | Spark scale · unknown · "real time", no numbers | no / unknown / unknown | score, uncalibrated · LR prob · unknown | no · threshold · product states | AGPL · MIT (last release 2024-08) · commercial | Senzing "community Rust" **UNVERIFIED** — no repo found |
| Streaming ER research: SPER ([arXiv 2512.23491](https://arxiv.org/html/2512.23491)), DaWaK 2025 stream-embedding ER (paywalled) | progressive / incremental ER | SPER 3–6× faster than progressive baselines | research | no | n/a | unknown | no code checked |

**Judgment:** the stack for `s2w`'s entity match is already implied by H: exact hashes and LSH
per event → a Fellegi–Sunter posterior (Splink today, GoldenMatch if its Rust kernels mature)
over H's candidate pairs → a decision model only on the indeterminate band, on streams where
names and values are readable. Contract B4's entity floor is measured on H alone first.

---

## 5. Candidates: hosted low-latency APIs, and calibration / conformal wrappers

### 5a. Hosted APIs

Cost per 1,000 calls assumes 300 input / 10 output tokens. TTFT figures are Artificial Analysis
72-hour medians on a medium prompt, so they overstate a 300-token call.

| Provider · model | Task shape | Latency (published) | $/1k calls | Logprobs / scores | Pinning | Note |
|---|---|---|---|---|---|---|
| **TypeSafe Jev** direct; **Cloudflare `typesafe/jev`**; Vercel AI Gateway; LangSmith (BYOK) | typed noul/choice/score | 70–500 ms e2e (vendor) | **$0.013** | calibrated probabilities + confidence | `jev-1.13.0`; policy unpublished | Cloudflare: 32k ctx, ZDR, no waitlist ([page](https://developers.cloudflare.com/ai/models/typesafe/jev/)) |
| **Kev 4B on OpenRouter** | same wire format | 18 ms H100 self-host | $0.013 (**UNVERIFIED** listing; page 404) | fitted temperature shipped | checkpoint | the same-price open fallback |
| **Together Tev1-4B-experimental** | 2–24-option choice, one letter | unpublished | $0.013 | token logprobs, README: "not calibrated confidence" | experimental | [repo](https://github.com/togethercomputer/tev1) |
| Cerebras gpt-oss-120b | chat + JSON schema | AA TTFT 1.66 s (reasoning) | $0.113 | **yes**, top_logprobs ≤ 20 | unknown | no ≤ 8B shared model ([docs](https://inference-docs.cerebras.ai/api-reference/chat-completions)) |
| Groq gpt-oss-20b | chat + strict JSON schema | AA TTFT 2.90 s (reasoning); 950 t/s | $0.026 | **no** (HTTP 400) | undated ids; 8B model now enterprise-only | ([docs](https://console.groq.com/docs/openai)) |
| OpenAI gpt-5-nano / 5.4-nano | chat + structured output | AA TTFT 0.81 s (minimal) / 0.67 s | $0.019 / $0.073 | not listed on 5.x nano | dated snapshots (`-2026-03-17`) | residency +10% ([pricing](https://developers.openai.com/api/docs/pricing)) |
| Gemini 2.5 Flash-Lite / 3.x Flash-Lite | chat + JSON | AA TTFT 0.29 s / **9.3 s** (3.5, reasoning on) | $0.034 / $0.09–0.115 | 2.5 yes; **3.x removed, "working as intended"** ([forum](https://discuss.ai.google.dev/t/missing-logprobs-support-in-the-newest-gemini-models-3-1-pro-3-6-flash-on-vertex-ai-and-ai-studio/176557)) | 2.5 access now restricted | |
| Claude Haiku 4.5 | chat + tool schema | AA TTFT 0.65 s | $0.35 | **no** parameter | `-20251001`, ≥ 60 d notice | 25× the nano tier ([API](https://platform.claude.com/docs/en/api/messages)) |
| Mistral Classifier API 8B / 3B | fine-tuned per-label scores | unpublished | $0.012 / $0.03 + $2/model/month | native scores | your model | docs page "deprecated", pricing live — confirm before building ([pricing](https://mistral.ai/pricing/api/)) |
| AWS Comprehend custom (real-time) | trained classifier | unpublished | **≈ $6** + $1.80/h per idle IU | native scores | your model | 60 s minimum billing; regional ([pricing](https://aws.amazon.com/comprehend/pricing/)) |
| Cloudflare distilbert-sst-2-int8 | binary sentiment only | unpublished | $0.008 | scores | id | no zero-shot NLI model in the catalog |
| Jina Classifier | zero-shot ≤ 256 classes; few-shot ≤ 16 | unpublished | unknown (token-metered) | scores | model name | [page](https://jina.ai/classifier/) |
| Galileo Luna-2 | True/False judge | 152 ms avg | $0.006 | logprob-derived | — | **enterprise platform only, no standalone API** |
| Patronus Lynx / GLIDER | eval judge | unpublished | **$10–20** (+$10 explanations) | score + pass/fail | — | three orders of magnitude above the floor |
| Fireworks < 4B / Together 8B / Baseten | small open models | Fireworks 200–500 ms TTFT (**UNVERIFIED**) | $0.031–0.062 | typically yes | — | |
| Not Diamond / Martian | LLM routers | +100–150 ms / "5–15 ms" (**UNVERIFIED**) | $0.015 fee + model | n/a | n/a | routers of prompts, not judgment engines |
| Cohere Classify · Atla Selene API | — | — | — | — | — | retired 2025-09-15 · site 404, API "no longer active" (**UNVERIFIED**) |

Two findings: **logprobs are disappearing from the cheap hosted LLM tier** (Groq 400, Claude
none, Gemini 3.x removed, OpenAI 5.x nano unlisted), so a hosted LLM cannot give `s2w` a
probability at all unless it is Cerebras or a classifier-shaped API; and **the price floor for a
300/10 call is ~$0.01–0.02 per 1,000**, set jointly by Jev, Kev, Tev1, Mistral's classifier and
gpt-5-nano, with evaluation-model vendors three orders of magnitude above it.

### 5b. Calibration and conformal wrappers

| Wrapper | Guarantee | Online / under shift | Rust | Licence · source |
|---|---|---|---|---|
| Temperature scaling (netcal 1.4; what Kev, Laya, Verdict, Rune, mpuig/system-one actually ship) | none (parametric) | refit on a window; mpuig fits per workload at runtime from ~100 labels, confident-error rate 14–19% → 3–4% | trivial (one scalar) | Apache-2.0 ([netcal](https://github.com/EFS-OpenSource/calibration-framework), [mpuig](https://github.com/mpuig/system-one)) |
| Venn–Abers (`venn-abers`, also in MAPIE) | one of (p0, p1) is perfectly calibrated, distribution-free; **width p1 − p0 is a native abstain signal** | windowed refit | no | MIT ([repo](https://github.com/ip200/venn-abers), [UAI 2014](https://www.auai.org/uai2014/proceedings/individuals/166.pdf)) |
| MAPIE 1.5 | coverage ≥ 1 − α under exchangeability; conformal risk control incl. an LLM-as-judge recipe; exchangeability tests | shift *detection*; adaptive CP present, streaming API unclear | no | BSD-3 ([docs](https://mapie.readthedocs.io/en/latest/)) |
| TorchCP (JMLR 2025) | coverage; **ACI** long-run coverage on arbitrary streams (regression-oriented); conformal LM | yes (ACI) | no | LGPL-3.0 ([repo](https://github.com/ml-stat-Sustech/TorchCP)) |
| crepes 0.9.1 | Mondrian per-class coverage; conformal test martingales | martingale shift test | no | BSD-3 ([repo](https://github.com/henrikbostrom/crepes)) |
| **`wm-conformal` 9.2.8** (2026-09-24) | split conformal sets, APS; finite-sample marginal coverage | no ACI | **yes** | MIT; no paper, unknown adoption ([docs.rs](https://docs.rs/wm-conformal/latest/wm_conformal/)) |
| `conformal-prediction` 2.0.0 (ruvnet) | regression-only; claims streaming and drift | claims | yes | MIT/Apache; marketing-heavy, unproven |
| **ACI** and follow-ups: Blackwell-approachability ACI ([arXiv 2510.15824](https://arxiv.org/abs/2510.15824)); **ACI under delayed feedback** ([arXiv 2609.07251](https://arxiv.org/abs/2609.07251), 2026-09-07) | long-run coverage on non-exchangeable streams; the delayed-feedback variant carries an explicit dependence on delay τ | **yes** | ~20 lines to hand-roll | papers |
| Selective / abstaining judges: SCOPE ([arXiv 2602.13110](https://arxiv.org/abs/2602.13110), ICML 2026); Judge-Retrieve-or-Abstain ([arXiv 2608.17994](https://arxiv.org/abs/2608.17994)) | error / FDR among *accepted* verdicts ≤ α; black-box | batch calibration | no | papers, no code found |
| Feasibility of conformal risk control for LLM outputs ([arXiv 2606.29054](https://arxiv.org/abs/2606.29054), rev. 2026-09-07) | if baseline risk μ > target α, any distribution-free method must reject ≥ (μ − α)/(M − α) of inputs | re-check under shift | — | **the abstention rate is lower-bounded by the engine's error rate; no wrapper fixes a bad judge** |

**Judgment:** the wrapper stack for a persisted-verdict pipeline is: a per-engine, per-stream
temperature (or Venn–Abers) on the reported-answer probability → a split-conformal or
FDR-style acceptance threshold → ACI, in its delayed-feedback form, because `s2w`'s outcomes
arrive late by design (contract A6). Only the middle step has a Rust crate; the ACI step is
trivial. ⚠️ Gate 4's A9.3 is an *overall* calibration ratio; these wrappers give coverage, not
that ratio, so they help the router (§8) more than the gate.

---

## 6. The trend, and what it means for `s2w`

**What happened in twelve days** (all dated above): a closed API defined a wire format; the
format was reimplemented on stock LLMs (SemIf, AnyJev, TypeSafe's own adapter), on 0.6B–35B
fine-tunes (Kev, Decider, Tev1, Intern-Decision, JevK5, Reflex), on 144M–421M encoders (Laya,
Julia-1, Verdict, von, GLiNER2.5-Decide), and on a 7.9M head with a C runtime (Blink); training
costs quoted were **$17** (Tev1), **$104.08** (Julia-1), 91 minutes on one H100 (Kev), 8.8 hours
on a GTX 1660 Ti (Verdict); two aggregators, a directory, a reproducible benchmark and three
calibration harnesses appeared; two gateways added the model class as a first-class type. The
pre-Jev classifier-API tier (Cohere Classify, Mistral's factory) had just closed, which left the
niche empty. HN's top questions were "so, a classifier?" and "why not XGBoost on my data?"
([HN](https://news.ycombinator.com/item?id=49717558)), and on the one synthetic benchmark where
someone ran the answer, TF-IDF + logistic came within 11 points of Jev at ~8 ms (§2b).

**Judgment on "a new type of frontier lab".** The frontier here is not capability; the open
encoders are within a few points of Jev on typed decisions and ahead on some sets. It is
*calibration under distribution shift, at a published p99, with a stable pinned id* — the
software properties TypeSafe's primer names ("structure, reliability, observability,
testability, speed, consistency, and low cost"). No lab, TypeSafe included, has published the
calibration half. What a $104 training run does prove is that a decision model tuned to *one
stream's* typed questions is now a weekend of compute, which turns "which model" into "which
training set" — and `s2w` will own the best possible training set for its own stream: persisted
verdicts with late-arriving outcomes (§8e).

**What it means for the architecture.** Dave's framing is right: the router must swap engines
as they appear. Concretely: (1) one adapter for `/v1/systemone` (HTTP for hosted and sidecar
engines) plus one in-process FFI adapter (Blink-class C, or an `ort` encoder), both behind the
`Engine` trait; (2) an engine is *registered* with a name, a pinned version, a measured latency
profile and a calibration temperature, never hard-coded; (3) the ledger scores per engine
version, so a new engine earns its rung by track record (the README's "each predictor's record
… is what the System 1 router will use") rather than by launch post. The category will churn;
the trait, the ledger and the adapter are what does not.

---

## 7. Mapping to `s2w`

### 7a. The engine trait, and where abstain comes from

No decision model returns `Abstain`; the adapter derives it. Rules that survive every model in §2:

1. **Abstain on input, before the call.** If the question reads a field and the field is a hash
   (obfuscated stream, B2.2) or empty → `Abstain{reason = no_readable_input}`. This is the rule
   decision 0010 already fixes for H's name embeddings; a decision model is the same kind of reader.
2. **Abstain on the distribution, after the call.** Gate on the probability of the reported
   answer through the engine's fitted temperature; never on a vendor `confidence` field (the
   formulas differ, Verdict 4). Southbridge's 0.53 near-tie is the fixture to write.
3. **Abstain on the instrument.** Timeout, 429, or a response `model` id that is not the pinned
   one → `Abstain{reason = engine_unavailable | model_drift}`. Contract A6 already treats a missed
   deadline as a base-rate guess.
4. **Persist the id and the temperature.** Every `Propose` carries the versioned model id
   (Jev `model` field; Laya checkpoint SHA-256; Kev/Rune HF revision; Blink file hash) and the
   temperature that was applied, so replay is exact and the ledger can split skill by engine
   version.

### 7b. Rung by rung

| Rung / role | Budget | Fits | Does not fit | Why |
|---|---|---|---|---|
| System 1 rung 1: rules | µs | code | every model | most events need no judgment |
| System 1 rung 2: local hashes / LSH / embeddings | µs–ms | H's containment and alias tests; `gaoya`/`probminhash` | — | already in H |
| **System 1 rung 3a: in-process form decision** | < 1 ms | **Blink-tiny**; a logistic head over an embedding H already computed | anything that reads text | the only per-event engines; "which of these 4 known types does this event's shape point at", not names |
| **System 1 rung 3b: sidecar encoder decision** | 20–50 ms, sampled or GPU-batched | Laya (ONNX INT8), Julia-1, Verdict, GLiNER2.5-Decide, von, Opir-edge | Jev/Kev at this budget | 2–11/s per CPU core; ~100–300/s batched on a T4 |
| **System 1 rung 3c: hosted decision** | 100–500 ms, async, budgeted | Jev; Kev via OpenRouter; Cloudflare-hosted Jev | per-event use at 1,000 ev/s (20 req/s unbatched; $12.60 per million events) | batch only when the question is per-batch |
| System 2: type proposal | seconds | LLM (as designed); a decision model as a *checker* of a proposed mapping (Choice over candidate roles per field) | decision models as proposer | they pick among options code lists; they cannot propose one |
| **System 2: entity match, indeterminate band** | seconds per pair | Splink-class F-S for the bulk; Jev / Kev / Rune with a three-level Score + field Nouls on the band | any of them on the obfuscated stream | reads names; rule 1 applies |
| System 2: merge review | seconds | same; Rune's opt-in thinking for the 0.4–0.6 band | — | a merge that later splits is a false merge (README); escalate the middle |
| Anomaly flag | ms–s | Blink / an encoder with a fixed label set; `noul` "out of pattern for its entity" | Jev for numeric anomalies | "not a calculator", dates as text |
| **Gate-4 forecast** | before cutoff | any decision model as a *reported* predictor arm (A5 forbids revert scores as primary-arm input) | as primary input | text-rich, independent outcomes, base rate 3.8%; A9.3 measurable per engine |

### 7c. Where Jev is strongest, and where something else wins

- **Jev wins** on reading comprehension per millisecond at zero ops: the highest index score,
  a pinned id, a documented parallel-questions contract, an SDK retry policy, and now three
  hosts. For one hosted judge on the indeterminate entity band, it is the default; Kev on
  OpenRouter is the same-format, same-price fallback.
- **Kev wins** where the verdict must be reproducible on our hardware with a public calibration
  curve (ECE 0.042, coverage-at-error published, Apache-2.0), given a 9 GB GPU.
- **Julia-1 wins** where the engine must run on a CPU with native abstention and a multilingual
  base for ~$0 — at 89–294 ms it is a sampled rung, and its 64% on Banking77 shows long option
  lists are its weakness.
- **Laya wins** for a CPU-only laptop deployment with the widest runtime surface (ONNX INT8,
  MLX, CoreML, laya.cpp) and the only published warning that an English checkpoint is
  confidently wrong off-script.
- **Blink wins** the only per-event slot, by being tiny and honest about not reading.
- **A trained classical model wins** wherever labels exist: TF-IDF + logistic at 66.1% vs Jev
  72.7% at ~8 ms and ECE 0.021; gate 4's B1 is exactly that arm.
- **Nothing here beats H on structure.** Decision models pick among options code names; H
  discovers the options. H, then S2, then a judge over H's ambiguous pairs — unchanged.

---

## 8. Routing beyond latency

Dave's question: is one engine better suited to classification and another to something else,
and is routing on that a research question? Yes on both counts.

### 8a. Dimensions a System 1 router should route on

| Dimension | What varies between engines | Evidence |
|---|---|---|
| **Judgment kind** | classify / extract / entity-match / rank / anomaly / probability-for-forecast; option-list length | Julia-1 94% AG News vs 64% Banking77 (72 options); GLiNER2.5-Decide leads 9/17 datasets and loses 8; Jev's four workflows span 61.8–76.0%; Blink reads form only; CLM is trained on agentic trajectories and tool-calling |
| **Latency budget** | 54 µs → 500 ms; CPU vs GPU; batchability | §2 |
| **Language and script** | EN-only checkpoints fail confidently off-script | Laya's Khmer case; Jev "CJK not equally well"; Julia-1 and mmBERT multilingual by construction |
| **Calibration quality on *this* stream** | raw ECE 0.16–0.47 before a fitted temperature; Jev 0.063–0.281 by dataset | §2b harnesses |
| **Abstain shape** | native (Julia-1), threshold + coverage curve (Kev), none (Jev), thinking-on-doubt (Rune) | §2 |
| **Cost** | $0 local vs $0.013 per 1,000 hosted vs $10–20 for judge vendors | §5a |
| **Measured accuracy per stream and per engine version** | the ledger's own record | README: "each predictor's record … is what the System 1 router will use" |
| **State shape** | prose vs JSON vs numeric vs hashed | Jev's jaggedness page; obfuscation makes every reader abstain |

### 8b. Prior art on how

- **Cascades** — cheap engine first, escalate on low confidence or abstain. FrugalGPT
  ([arXiv 2305.05176](https://arxiv.org/abs/2305.05176), 2023) and AutoMix
  ([arXiv 2310.12963](https://arxiv.org/abs/2310.12963)) are the canonical LLM versions; the
  decision-model world already runs the pattern in production: Southbridge's Jev → Luna → Gemini
  review chain, Rune's thinking-below-0.7, TypeSafe's own "confidence-gated routing" pattern
  ([docs](https://docs.typesafe.ai/patterns/confidence-routing.md)). 2026 work characterises
  *when* escalation pays ("Is Escalation Worth It?",
  [arXiv 2605.06350](https://arxiv.org/pdf/2605.06350)) and learns the cascade policy without a
  separate quality estimator (RLCascadeRouter, [arXiv 2608.15817](https://arxiv.org/pdf/2608.15817))
  — both **UNVERIFIED** beyond title and abstract.
- **Learned routers** — a classifier picks the engine per input. RouteLLM
  ([arXiv 2406.18665](https://arxiv.org/abs/2406.18665)) and RouterBench
  ([arXiv 2403.12031](https://arxiv.org/abs/2403.12031)) for LLMs; Not Diamond and Martian as
  products (§5a); Knowledgator's SCX Router as a streaming zero-shot model-selection classifier
  (**UNVERIFIED** detail). Note the Decision Index dropped RouterBench "because its prompt gives
  away the best route" ([README](https://github.com/apolinario/decision-index)) — a warning about
  what a learned router actually learns.
- **Bandits over engines** — online, cost-aware, adapts to drift and onboards new arms at runtime:
  "Learning to Route LLMs from Bandit Feedback" ([arXiv 2510.07429](https://arxiv.org/pdf/2510.07429)),
  "Drift-Aware LLM Routing with Sparse Contexts and Shared Budgets"
  ([arXiv 2609.00662](https://arxiv.org/pdf/2609.00662), 2026-09), ParetoBandit (open source,
  dollar budgets, onboards models at runtime — **UNVERIFIED**, search summary). This is the
  shape that matches "swap engines as they appear".
- **Mixture of experts at the service level** — one wire format, many engines, a gate in front.
  The gateways (Vercel, LangSmith) do the plumbing but route by *model id you name*, not by
  judgment; nobody routes by judgment kind yet.
- **Per-engine selective classification** — SCOPE and Judge-Retrieve-or-Abstain (§5b) bound
  error among accepted verdicts per engine, which is the primitive a router composes.

**What none of it does:** route *typed judgments* (not prompts) to *decision engines* (not LLMs),
score them against *late-arriving stream outcomes*, and learn from a *second system's
corrections*. The closest analogue is Southbridge's hand-built chain.

### 8c. Candidate → judgment kinds it suits

| Engine | Classify (short list) | Classify (long list, 50+) | Extract / select a value | Entity match (pair) | Rank | Anomaly | Probability for a forecast |
|---|---|---|---|---|---|---|---|
| Blink-tiny | form-only, ≤ 4–16 options | no | no | no | menu ranking on cached state | shape-level | no |
| Julia-1 | yes (2–20 options by design) | weak (Banking77 64%) | no | untested | yes (built for it) | untested | untested |
| Laya / Verdict / von | yes | unknown | no | untested | yes | untested | untested |
| GLiNER2.5-Decide | yes; multi-label; "none" option | yes (label-set at call time) | via GLiNER2.5 family | untested | ordinal 0–10 | untested | untested |
| Kev / Decider / Intern-Decision | yes | yes (≤ 62 options, Intern; ≤ 255 Decider) | select among code-supplied spans | plausible (same shape as Jev) | yes | untested | untested; Kev's coverage curve is the closest published |
| Jev | yes | yes (≤ 255) | select, not generate (cookbooks) | **yes, documented** (cookbook, Southbridge) | yes (rerank cookbook) | weak on numbers | untested on independent outcomes |
| Rune | yes; images | yes | select | plausible | yes | untested | untested |
| CLM-8B | tool/action selection | unknown | no | no | actions | no | verifier on agentic tasks |
| Splink / F-S | no | no | no | **yes, calibrated by construction** | no | no | no |
| Encoder + logistic head | yes, if labelled | if labelled | no | if labelled pairs | no | if labelled | yes, if labelled (B1's shape) |

"untested" is literal: no page measured it.

### 8d. What this means for the router

Route on a **tuple**, not a scalar: `(judgment_kind, option_count, state_shape, language,
budget)` selects the eligible engines; the ledger's per-engine record on this stream orders
them; the fitted temperature and the abstain rule decide whether the chosen engine's answer
is *accepted* or the tuple is re-routed one rung up. The rung order in §7b is the static
cascade; the ledger makes it dynamic.

### 8e. The research question, stated for `s2w`

> **Can a System 1 router be learned from persisted verdicts plus System 2's corrections, and
> does it beat the static cascade?** Inputs: every `Propose{claims, confidence_bp, engine_id,
> temperature}` and `Abstain{reason}` in the log; every outcome the stream later reports (gate
> 4's ledger) and every accept/reject or later split that System 2 or Dave applies to a merge or
> a type (the "every judgment" row of the README's evaluation table). Policy class: a cost-aware
> contextual bandit over registered engines, with delayed feedback (the delayed-feedback ACI
> result gives the coverage half). Baselines: the static rung order of §7b; "always the highest
> index score"; "always the cheapest that does not abstain". Metric: skill per unit cost and per
> unit latency, with abstentions scored as base-rate guesses exactly as A6 already does.

**What gate 3 would need to measure it.** Gate 3 today compares H, H+S2 and B3 on structure
recovery (B1) and has no engine-selection axis. To measure routing it would need: (1) at least
two registered engines per judgment kind, so there is a choice to learn; (2) the persisted
verdict schema to carry `engine_id`, `temperature` and `reason` (a contract *schema* addition,
not a threshold change); (3) a frozen replay of the development window through each engine
alone, so the learned router's choice can be scored against every counterfactual — which the
pure fold and golden replay already make cheap; (4) an outcome source for System 1 judgments,
which today is only System 2's accept/reject, so the router is learning from a slower, not a
truer, judge until gate 4's ledger lands. **Judgment:** this is a gate-4-and-after question
dressed as a gate-3 one; gate 3 should only lay the schema down.

---

## 9. Gaps and unknowns

- Jev's size, architecture, training data and any calibration curve: undisclosed as of
  2026-09-27; CEO declines to discuss.
- p99 under load for every hosted engine: unpublished (Intern-Decision's p95 on one GPU is the
  only percentile any project prints). Jev's rate limits are explicitly unstable.
- Any decision model on JSON event state with hashed ids: no measurement found.
  `[no-record: decision-model latency/accuracy on event streams searched=typesafe docs, HN
  49717558, awesome-decision-models, awesome-jev-alternatives, Decision Index README,
  systemonemodels.org, X last 7 days (x-research CLI)]`
- Blink's licence and Blink-small's accuracy on real text; Julia-1's calibration; SemIf's
  licence (repo 404); Decider's "best calibrated" metric; Senzing's Rust support.
- Whether TypeSafe's RLCD equals the open reproductions' proper-scoring-rule fine-tuning.
- The Decision Index leaderboard page (401) — per-model rows are from summaries.
- Routing prior art from 2026 (RLCascadeRouter, escalation characterisation, drift-aware
  routing, ParetoBandit) — titles and abstracts only.

---

## 10. Ranked shortlist for a spike

Five, cheapest first, all through one `/v1/systemone` adapter (plus one FFI adapter for #1)
behind the `Engine` trait, on the gate-4 development window (English `recentchange`, 1,854
eligible edits in 30 minutes, base rate 3.8%) and on 10^4 events of the obfuscated stream.

**How the spike measures latency on hub.** For every engine: 10^4 events replayed from the
golden log at a fixed offered rate, one Rust harness (criterion for in-process, a `tokio` client
with per-call timers for HTTP), recording p50/p95/p99 per decision, decisions/s per core, and
the offered-rate at which p99 crosses 1 ms (in-process) or the engine starts returning 429 /
timeouts (hosted). CPU runs name the hub's CPU model and core count; GPU runs happen only if a
GPU is present and say which. ONNX INT8 exports (Laya, Julia-1, Verdict) run in-process via
`ort` so the sidecar tax is measured separately from the model.

| # | Engine | What the spike measures | Pass shape |
|---|---|---|---|
| 1 | **Blink-tiny, in-process** (C via FFI, or the WASM module) | per-event p50/p99 on one core at 1,000 ev/s; a 4-way "event type by shape" Choice vs H's stage-3 role label; ECE | p99 < 1 ms and agreement with H > 0.9, else it is not a rung |
| 2 | **Julia-1 and Laya, ONNX INT8 via `ort`, CPU** | decisions/s per core; p99; the 3-level Score over H's indeterminate entity pairs, plain stream, vs the answer key; abstain rate on obfuscated pairs (must be ~100%); Julia-1's native abstain vs Laya's `min_confidence` | ≥ 20 pairs/s per core and 0 confident answers on obfuscated pairs |
| 3 | **Jev 1.13.0** (pinned) | revert-in-30-min Noul as a *reported* predictor arm (A5 keeps it out of the primary arm); BSS vs B0/B1; the A9.3 ratio; $/1,000 events; 429 rate at 20 ev/s | BSS > 0 with the ratio inside [0.8, 1.25]; if not, the "calibrated decisions" claim fails its first independent-outcome test |
| 4 | **Kev-4B, local** (GPU only) | #3's questions on the same window; the Jev/Kev delta is the "does a public calibration curve transfer" question | within 0.02 BSS of Jev at zero API cost, else Jev stays the hosted default |
| 5 | **Splink-class F-S on H's pair candidates** | calibrated match probability from EM on the plain stream, no model; the number #2's engines must beat on the indeterminate band | if F-S alone clears the B4 entity floor, the decision model's job is the residue only |

What the spike does **not** do: put any model on the event path before #1 has a p99; use any
revert score as primary-arm input (A5); tune thresholds on the test window (A8 freeze); learn a
router (§8e) before two engines per judgment kind exist.

---

## Sources not reachable in full text

Marked **UNVERIFIED** where used: the Decision Index leaderboard page (401), SemIf's repository
README (404), von's README on its default branch (repo page fetched instead), Kev's OpenRouter
page (404; listing from OpenRouter's own post), the Vercel 5–18× figure (secondary report),
Kev's Mac latencies and Rune v2's RTX 5090 figure (search summaries), Blink's licence, Atla's
status (site 404), Martian's overhead, ParetoBandit, the 2026 cascade/escalation papers beyond
their abstracts, Fireworks TTFT.

## Design implications

1. Target the `/v1/systemone` wire format plus one in-process FFI adapter behind the `Engine`
   trait, with the four abstain rules of §7a in the adapter and an engine *registry* (name,
   pinned version, latency profile, temperature) instead of hard-coded engines. → deferred:
   issue to file against #51, ≥ 2 engines per adapter per the two-implementations rule.
2. README "System 1, decision models" row: replace "TypeSafe's Jev and similar models" with the
   three-tier reading of §7b (in-process form model / sidecar encoder / hosted) and cite this
   note; README "System 1 router" row: each rung names a latency budget *and* a judgment-kind
   tuple *and* an abstain reason (§8d). → deferred: README edit, same issue as (1).
3. Persisted verdicts carry `engine_id`, `temperature` and `reason` (§7a rule 4, §8e need 2). →
   deferred: contract schema addition as a dated section (the contract is frozen; this changes
   no threshold), same issue as (1).
4. Gate 4: one decision model as a reported, non-primary predictor arm; A9.3 per engine. →
   deferred: gate-4 epic #14.
5. Gate 3: decision models are not an arm; they may serve inside H+S2's System 2 as a pair judge
   over H's indeterminate entities on the plain and private streams only; gate 3 lays down the
   verdict schema of (3) and measures nothing about routing. → rejected as an arm (B1 fixes
   three arms; decision 0010's reasoning applies unchanged); → deferred as a System 2 component:
   gate-3 epic #13.
6. The learned-router research question (§8e) with its four preconditions. → deferred: a
   post-slice issue, labelled paper-candidate; not before gate 4's ledger exists.
7. Spike order of §10, Blink first. → deferred: one spike issue.
8. Watch list, not work: Julia-1's API pricing ($0.025/MTok planned), GoldenMatch's Rust kernels,
   Rune v3 GGUF, `wm-conformal`. → deferred: re-check at the gate-4 spike, not before.
