# Round 15 — Multiplayer networking

## Progress

- **Transport** (`misty_engine::network::transport`): connections over `std::net::UdpSocket`, read on a
  `misty-net` thread; three channels (reliable ordered, reliable unordered, unreliable), a connect
  handshake with a per-connection nonce, keep-alives (an empty packet after 0.1 s of silence), timeouts,
  a server-full refusal, fragmentation of messages over 1 KB (up to 4 MB), and a network simulator
  (latency, jitter, loss) on outgoing datagrams. Driven by the owner's clock (`Endpoint::update(now)`), so
  tests run on simulated time; `MemoryNet` replaces sockets in tests.
- **LAN discovery** (`network::discovery`): a listening server answers broadcast queries on UDP 47777 for
  its product name; `Network.FindLanGames()` / `Network.LanGames`.
- **Components** (Component ▸ Netcode): **Network Manager** (address, port, listen address, tick rate,
  player prefab, connection approval, max connections, timeout, scene management, LAN discovery, network
  simulator), **Network Object** (global id, destroy with owner), **Network Transform** (authority,
  sync position / rotation / scale, interpolation and its delay, thresholds, snap distance, predict).
- **Session** (`network::session`): host / server / client modes, approval (`Hello` → `Welcome` or
  `Denied`), scene sync (`Welcome` names the server's scene; clients load it, report `SceneReady`, get every
  object, then `SyncDone`; a server scene change sends `LoadScene`), spawning with owners (prefab copies by
  path, scene objects by global id), despawn, ownership changes, per-second stats.
- **Script host** (`scripting::network_api`): `OnNetworkSpawn` / `OnNetworkDespawn` / ownership callbacks,
  Network Variables (`NetworkVariable({ read, write, onChanged })` in `_attributes`), RPCs (`…ServerRpc` /
  `…ClientRpc` names or `ServerRpc({...})` / `ClientRpc({...})` attributes, `RpcTarget`), network properties
  on scripts (`self.IsOwner`, …), transform interpolation, client-side prediction with reconciliation,
  Rigidbodies made kinematic on peers that don't move them.
- **Lua API**: `Network.StartHost/StartServer/StartClient/Shutdown`, `IsServer`, `IsClient`, `IsHost`,
  `IsListening`, `IsConnectedClient`, `LocalClientId`, `ServerClientId`, `Mode`, `ConnectedClientsIds`,
  `LocalPlayer`, `GetPlayerObject`, `GetNetworkObject`, `Spawn`, `DisconnectClient`, `connectionData`,
  `ConnectionApproval`, `SenderClientId`, `FindLanGames`, `LanGames`, events `OnServerStarted`,
  `OnClientStarted`, `OnServerStopped`, `OnClientStopped`, `OnClientConnected`, `OnClientDisconnected`;
  `GetComponent("NetworkObject")` (`Spawn`, `SpawnWithOwnership`, `SpawnAsPlayerObject`, `Despawn`,
  `ChangeOwnership`, `RemoveOwnership`, ids and flags), `GetComponent("NetworkTransform")` (`Teleport`,
  `CanCommitToTransform`, `interpolate`).
- **Runtime / player**: `Runtime::network_start / network_shutdown / network_stats / take_network /
  adopt_network`; scene loads in the editor, the player and `SaveSystem.Load` keep the session;
  `Request::NetworkLoadScene` loads any project scene. The player takes `--net-host`, `--net-server`,
  `--net-client addr`, `--scene`, `--scene-file`, `--title`, `--size`, `--pos`, and shows the mode and RTT in
  its window title.
- **Editor**: Inspectors for the three components (the Network Manager's has Start Host / Server /
  Client and Shutdown while playing, the Network Object's shows id / owner, simulator edits apply live);
  **Window ▸ Multiplayer ▸ Multiplayer Play Mode** (Players 2–4; Play hosts in the editor and launches
  `misty_player` windows as clients on localhost with the scene as it was at Play; they close on Stop;
  remembered in `Library/MultiplayerPlayMode.json`); a two-line network overlay in the Game view (RTT,
  loss, packets and KB/s in and out); Component ▸ Netcode and Ctrl+K entries.
- **Sample**: `Scenes/NetworkDemo.scene` from `sample_network.rs` (`network_demo_matches_its_generator`,
  `network_demo_plays_host_and_client`).
- **Tests**: transport (handshake, loss + jitter with ordered / unordered delivery, fragmentation, timeouts,
  goodbye, server full, latency, real UDP, sequence wrap-around), discovery, protocol round trips,
  interpolation buffer / playback clock / reconciliation, session approval; in-process host + clients
  (spawn, variables with permissions and callbacks, RPCs both ways with targets, owner-only reads,
  interpolation, owner authority, ownership change, prediction with a disagreeing server, polite
  disconnect, timeout, a lossy slow network, a real UDP session), the Lua API (start, approval with a
  refused payload, events, shutdown), the player's command line and scene carry-over, kittest for the
  Inspectors and the Multiplayer Play Mode window.
- **Docs**: `docs/scripting.md` ▸ Networking.

## Decisions

1. **Own transport over `std::net::UdpSocket`**, not a crate. `laminar` is unmaintained; `renet` /
   `renet_netcode` are maintained and MIT/Apache but bring their own netcode model, authentication tokens
   and channel configuration we would wrap anyway; `quinn` needs tokio. The transport is under 1,500 lines with
   its tests, has no dependencies, runs on the caller's clock (deterministic tests with an in-memory link),
   and does exactly the three channel kinds the brief asks for.
2. **Reliability per message on per-packet acks**: each packet carries `seq`, `ack` and a 32-bit ack
   history; reliable messages stay queued until a packet that carried them is acknowledged and are resent
   after 1.5 × RTT (30 ms – 1 s). Fragments are separate reliable messages with consecutive ids (ordered
   delivery reassembles them naturally); unreliable fragments arrive whole or not at all (2 s window).
3. **Naming** follows the reference package's shape: `Network` (the manager singleton), `StartHost`,
   `IsServer`, `LocalClientId`, `ConnectedClientsIds`, `NetworkObject` / `NetworkTransform` components with
   `Spawn`, `ChangeOwnership`, `OnNetworkSpawn`, `OnGainedOwnership`, `ServerRpc` / `ClientRpc` suffixes,
   `RequireOwnership` as `requireOwnership`, `NetworkVariable` read/write permissions. Misty-specific:
   `Network.Spawn(prefab, position, owner)` as a one-call spawn, `RpcTarget` as an optional last argument
   (instead of RPC params structs), `connectionData` as text, `Network.FindLanGames`.
4. **Network Variables are plain script fields** marked in `_attributes`; changes are found by comparing
   with the last synchronised value at each tick (no proxies, so `self.score = self.score + 1` just works).
   Writes by peers without permission are undone with a warning instead of an exception.
5. **RPC calls go through per-instance wrappers**: instances of classes with RPCs get a closure of the same
   name that routes the call (local on the server / host, or sent); receivers call the class's original
   function. Scripts of network objects get a function `__index` so `self.IsOwner` etc. read live state.
6. **Behaviours are identified by index** (scripts of the object's subtree in Hierarchy order, then slot),
   identical on every peer because they spawn the same prefab or scene object.
7. **Scene objects match by global id** (a GUID in the Network Object, refreshed on copies like Save IDs;
   repeated ids get `#n` by Hierarchy order); prefab copies are rebuilt from their prefab path, so only
   prefabs (or scene objects) can be spawned. In-scene objects the server no longer has are destroyed on
   the client after the initial sync.
8. **Transform interpolation uses a playback clock per sender** (local time nudged towards the sender's
   timestamps) and plays `interpolation_delay` behind it; teleports beyond `snap_distance`.
9. **Prediction without re-simulation**: the owner keeps its pose per local frame; server states carry the
   frame of the owner's last input RPC they include (`ack`), and the owner adds the difference between the
   server pose and its recorded pose for that frame to its current pose (equivalent to replaying additive
   moves). No input reached the server yet (`ack` 0): only a teleport moves the owner.
10. **The session lives in the script world and moves between runtimes** on scene loads
    (`take_network` / `adopt_network`), so one connection survives `SceneManager.LoadScene`, scene sync
    and `SaveSystem.Load`. Lua listeners and the approval callback belong to the scene's Lua state.
11. **Multiplayer Play Mode launches the player executable** rather than extra in-editor game views: real
    separate processes behave like real clients (own input, window, audio), and the player already renders
    everything. The editor hosts; players get `--scene` + `--scene-file` with a copy of the scene at Play.
12. **Stats** (RTT, packets and bytes per second, loss) are computed once a second from the endpoint's
    counters; the Game view shows two lines bottom-left only while a session runs (toggle in the window).

## Left

- Relay servers / NAT punch-through (players on different networks need a reachable host), encryption
  and authentication tokens.
- Dedicated-server builds (a headless player without a window or GPU).
- Network Animator, Network Rigidbody (beyond the kinematic switch), parenting of spawned objects,
  observers / interest management, per-client visibility.
- Bandwidth: quantised positions / smallest-three rotations, message batching across objects, string
  interning for script and field names.
- Lua listeners and the approval callback don't survive scene loads (re-register in `Start`).
