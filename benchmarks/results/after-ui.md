| phase (median latency; FPS) | 100 comp. | 500 comp. | 2000 comp. | 5000 comp. |
|---|---:|---:|---:|---:|
| startup (Workspace::new) | 0.0 ms | 0.0 ms | 0.0 ms | 0.0 ms |
| schematic: first frame (scene build + GPU init) | 210.6 ms | 175.5 ms | 228.7 ms | 412.1 ms |
| schematic: steady frame (cached scene) | 1.3 ms (p95 1.5 ms, 769 fps) | 1.5 ms (p95 1.9 ms, 671 fps) | 1.3 ms (p95 1.7 ms, 730 fps) | 1.7 ms (p95 3.7 ms, 544 fps) |
| schematic: zoom (wheel ± + frame) | 2.1 ms (p95 2.4 ms, 476 fps) | 1.4 ms (p95 1.8 ms, 704 fps) | 6.4 ms (p95 7.1 ms, 154 fps) | 15.5 ms (p95 18.5 ms, 64 fps) |
