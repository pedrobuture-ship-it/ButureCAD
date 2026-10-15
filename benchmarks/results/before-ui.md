| phase (median latency; FPS) | 100 comp. | 500 comp. | 2000 comp. | 5000 comp. |
|---|---:|---:|---:|---:|
| startup (Workspace::new) | 0.0 ms | 0.0 ms | 0.0 ms | 0.0 ms |
| schematic: first frame (scene build + GPU init) | 239.8 ms | 171.8 ms | 228.0 ms | 416.4 ms |
| schematic: steady frame (cached scene) | 1.3 ms (p95 1.7 ms, 718 fps) | 1.3 ms (p95 1.7 ms, 724 fps) | 1.7 ms (p95 2.5 ms, 590 fps) | 1.4 ms (p95 1.6 ms, 704 fps) |
| schematic: zoom (wheel ± + frame) | 2.1 ms (p95 2.7 ms, 473 fps) | 1.3 ms (p95 1.6 ms, 756 fps) | 6.7 ms (p95 12.6 ms, 136 fps) | 22.5 ms (p95 27.1 ms, 45 fps) |
