# RFC: AAA-12 pixel budget

Status: accepted for Era 2 kitbash PBR
PR: clustered forward+ presenter (`klotho-render`)

This RFC closes the **pixel budget** for AAA-12. The lighting technique
is not in scope: Q8 already chose cook-baked irradiance probes + SSGI,
and this PR picks **clustered forward+** (not deferred).

## Technique (closed)

- **Clustered forward+** with a CPU tile list. The unlit+lambert family
  stays a separate pipeline object so Hearth 640×360 goldens do not move.
- Kitbash meshes are position-only (12-byte stride). Deferred would be a
  paper G-buffer of derived normals; forward+ punctual lights + one
  directional sun + CSM is enough for Era 2 kitbash scale.
- **IBL** is a procedural upper-hemisphere + sun. Cooked cubemap blobs
  are not a CAS kind yet (AAA-14).
- **GI** is cook-baked irradiance probes + SSGI. No SDF volume. GI is
  presenter-only and is never written to Trace.
- Cooked SH / 3D irradiance blobs land with AAA-14. AAA-12 samples the
  Manifest `ProbeGrid` transform and uses a presenter-owned constant
  irradiance volume when the grid is ready. A missing or invalid probe
  blob is skipped (fail open on pixels).

## Permutations

Lighting permutation is `VisualManifest.post`, not `GpuBudget`.

| Permutation | Cascades | Probes | SSGI | TAA | Bloom | Present gate |
| --- | ---: | --- | --- | --- | --- | ---: |
| Unlit (`PostFlags::UNLIT`) | 0 | no | no | no | no | ≤ 7 ms (Hearth) |
| Adventure | 3 | yes | yes | trivial | half-res extract | ≤ 11 ms 1080p high |
| Competitive | 1 | no | no | no | no | ≤ 8 ms 1080p |

Competitive **forces** GI off and one cascade even if `post.gi` is set.
Unlit does not allocate or sample shadow maps.

## Probe density

- Default spacing: **2000 mm**.
- Grid `dim` is `(u8, u8, u8)`. Era 2 cap: product **≤ 8×4×8 = 256**
  cells. Larger or empty dims, non-positive spacing, or a missing CAS
  blob → skip that grid.
- At most one ready grid is bound per frame.

## SSGI

- Half-res, **4** screen-space samples, composited after opaque.
- Skipped when `!post.gi`, when competitive, or when the previous
  present exceeded the budget (see fail-open).
- Cost is intended to stay well under the 11 ms adventure gate on a
  reference desktop; there is no locked GPU in this repo, so CI does
  not treat a local overrun as a test failure.

## Cascades

- Adventure: 3 splits (practical-split, same near/far as the camera).
- Competitive: 1.
- Unlit: 0.
- Shadow maps are depth-only over the same uploaded kitbash meshes.
  Resolution: 1024², 3 layers allocated for the PBR path only.

## Clustered lights

- Punctual cap: **32** point lights. Overflow drops extras.
- Tile grid: 16 px tiles, clamped to **32×18** so the mask uniform fits
  `downlevel_defaults` (16 KiB). Uniform array + CPU bitmasks; no
  storage buffers, no native-only features.
- A light whose screen-space circle misses a tile is not in that tile.

## 1080p gates

| Profile | `us_present` | `max_clusters` |
| --- | ---: | ---: |
| `GpuBudget::HEARTH` | 7_000 | 256 |
| `GpuBudget::AAA_ADVENTURE` | 11_000 | 2048 |
| `GpuBudget::AAA_SHOOTER` | 8_000 | 1024 |

`us_extract` stays 1_500 µs on all three; extract is not this RFC.
Budget is time + cluster cap. It does not select the lighting permutation.

## Fail open (pixels, never a kernel reject)

1. Drop-farthest clusters (`draw_list`, already landed).
2. Skip SSGI.
3. Drop extra cascades (adventure 3 → 1).

Steps 2–3 use the previous frame's `WgpuPresenter::last_present_us`
against `GpuBudget.us_present`. Over-budget never rejects a commit.

## Honest CI

There is **no locked reference GPU** in this repo. The gate is:

- constants on `GpuBudget` matching the HLD table
- CPU permutation / tile / probe-skip tests
- optional GPU timing recorded on `WgpuPresenter::last_present_us`

GPU tests skip when `try_headless()` is `None`. When they run they
assert draw/cascade outcomes. They do **not** fail CI if a present
exceeds 11 ms on the machine that happened to run them.

## This PR vs follow-ups

- **AAA-13:** GPU skinning; skinned instance lists are skipped, not
  crashed on.
- **AAA-14:** glTF UVs/tangents and cooked probe SH / IBL cubemaps.
- Decals / one-shot meshes are on the Manifest (AAA-11b) but are not
  drawn by this presenter.
- TAA is a 1-pixel neighborhood blend with the previous color, not a
  motion-vector resolve.
- No mesh shaders, no virtualized geo, no Lumen.
