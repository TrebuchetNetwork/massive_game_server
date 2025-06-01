## Possible improvement towards the world record ##



# 200vs200 high interaction times

Total: 38.29ms

- Input/AI (Stage 1): 18.43ms

- Physics (Stage 2a): 0.40ms

- Game Logic (Stage 2b): 0.04ms

- State Sync (Stage 3a): 19.41ms

- Broadcast (Stage 3b): 0.00ms (timed_out: false)

(Target Tick: 16ms)



# 200vs200 bots idle times

Total: 33.97ms

- Input/AI (Stage 1): 33.03ms

- Physics (Stage 2a): 0.19ms

- Game Logic (Stage 2b): 0.03ms

- State Sync (Stage 3a): 0.70ms

- Broadcast (Stage 3b): 0.03ms (timed_out: false)

(Target Tick: 16ms)





# 1000vs1000test on mac m2 max high interaction small map

Starting game loop...

2025-06-01T13:44:50.664877Z  INFO massive_game_server_core::server::game_loop: Game loop started. Tick rate: 33ms, Delta time: 0.033333335s

2025-06-01T13:44:50.666181Z  INFO massive_game_server_core::server::game_loop: Game loop running - Frame: 0

2025-06-01T13:44:50.667688Z  INFO massive_game_server_core::server::instance: Match starting! Mode: TeamDeathmatch

2025-06-01T13:44:50.667837Z  INFO massive_game_server_core::server::instance: [Bot Management] Attempting to spawn 1980 additional bots...

2025-06-01T13:44:50.742078Z  WARN massive_game_server_core::server::instance: Stage 2 (Physics/Logic) exceeded soft budget 12ms frame=0 ms=74.849 physics_ms=0.456 game_logic_ms=74.392

2025-06-01T13:44:50.744021Z  WARN massive_game_server_core::server::instance: Frame 0 timing breakdown:

Total: 77.79ms

- Input/AI (Stage 1): 0.99ms

- Physics (Stage 2a): 0.46ms

- Game Logic (Stage 2b): 74.39ms

- State Sync (Stage 3a): 1.91ms

- Broadcast (Stage 3b): 0.02ms (timed_out: false)

(Target Tick: 16ms)

2025-06-01T13:44:50.744027Z  WARN massive_game_server_core::server::instance: Tick processing WORK exceeded hard budget (game_loop will log wall-clock overrun) frame=0 ms=77.787 target=16

2025-06-01T13:44:50.744045Z  WARN massive_game_server_core::server::game_loop: Frame 0 took too long: 79.141833ms

2025-06-01T13:44:51.576205Z  WARN massive_game_server_core::server::game_loop: Frame 6 took too long: 743.309209ms

2025-06-01T13:44:51.579351Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:52.352714Z  WARN massive_game_server_core::server::game_loop: Frame 7 took too long: 776.492792ms

2025-06-01T13:44:52.391762Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:52.911718Z  WARN massive_game_server_core::server::game_loop: Frame 8 took too long: 558.991958ms

2025-06-01T13:44:52.913770Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:53.411213Z  WARN massive_game_server_core::server::game_loop: Frame 9 took too long: 499.477083ms

2025-06-01T13:44:53.412124Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:53.907466Z  WARN massive_game_server_core::server::instance: Frame 10 timing breakdown:

Total: 496.24ms

- Input/AI (Stage 1): 0.10ms

- Physics (Stage 2a): 0.74ms

- Game Logic (Stage 2b): 0.07ms

- State Sync (Stage 3a): 495.32ms

- Broadcast (Stage 3b): 0.00ms (timed_out: false)

(Target Tick: 16ms)

2025-06-01T13:44:53.907485Z  WARN massive_game_server_core::server::game_loop: Frame 10 took too long: 496.257375ms

2025-06-01T13:44:53.908029Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:54.396634Z  WARN massive_game_server_core::server::game_loop: Frame 11 took too long: 489.144334ms

2025-06-01T13:44:55.000027Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:55.488903Z  WARN massive_game_server_core::server::game_loop: Frame 12 took too long: 1.092254208s

2025-06-01T13:44:55.491351Z  WARN massive_game_server_core::server::instance: Event processing loop safety break triggered in run_game_logic_update.

2025-06-01T13:44:56.093767Z  WARN massive_game_server_core::server::game_loop: Frame 13 took too long: 604.848916ms