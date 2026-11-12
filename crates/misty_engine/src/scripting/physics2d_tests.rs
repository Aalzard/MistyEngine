//! Lua 2D API tests: Rigidbody2D, Physics2D queries and gravity, 2D callbacks, SpriteRenderer.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    BoxCollider2D, CircleCollider2D, Collider2DCommon, Rigidbody2D, Scene, ScriptRef, Scripts,
    SpriteRenderer, Transform,
};
use crate::scripting::{Input, LogKind, ScriptHost};

fn project(name: &str, source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_physics2d_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts").join(name), source).unwrap();
    root
}

fn object(scene: &mut Scene, name: &str, pos: Vec2) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(pos.extend(0.0)));
    e
}

fn run(scene: &mut Scene, root: PathBuf, frames: u32) -> Vec<String> {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(scene, physics, root);
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
    }
    let logs = host.take_logs();
    for (kind, text) in &logs {
        assert_ne!(*kind, LogKind::Error, "{text}");
    }
    logs.into_iter().map(|(_, t)| t).collect()
}

const PLAYER: &str = r#"
local Player = { landed = false }
function Player:Start()
    self.rb = self.gameObject:GetComponent("Rigidbody2D")
    self.sr = self.gameObject:GetComponent("SpriteRenderer")
    Debug.Log("gravity", Physics2D.gravity.y < -9)
    Debug.Log("types", self.rb.bodyType, self.rb.mass, self.rb.gravityScale)
    local box = self.gameObject:GetComponent("BoxCollider2D")
    Debug.Log("box", box.size.x, box.isTrigger, box.attachedRigidbody ~= nil)
    local hit = Physics2D.Raycast(Vector2.new(0, -0.6), Vector2.down, 10)
    Debug.Log("ray", hit.gameObject.name, string.format("%.2f", hit.distance), hit.normal.y,
        hit.rigidbody == nil)
    Debug.Log("miss", Physics2D.Raycast(Vector2(0, -0.6), Vector2.down, 0.5) == nil)
    Debug.Log("overlap", Physics2D.OverlapCircle(Vector2.new(0, -3), 0.2).name,
        #Physics2D.OverlapCircleAll(Vector2.new(0, 4), 0.1))
    self.sr.flipX = true
    self.sr.sortingOrder = 3
end
function Player:FixedUpdate()
    self.rb.velocity = Vector2.new(1, self.rb.velocity.y)
end
function Player:OnCollisionEnter2D(c)
    if not self.landed then
        self.landed = true
        Debug.Log("landed", c.gameObject.name, c.contacts[1].normal.y > 0.9,
            c.relativeVelocity.y > 1, c.collider.name, c:GetContact(0).point.y < -2)
    end
end
function Player:OnTriggerEnter2D(other)
    Debug.Log("coin", other.gameObject.name, other:CompareTag("Untagged"))
    Destroy(other.gameObject)
end
return Player
"#;

#[test]
fn rigidbody2d_queries_callbacks_and_sprite_renderer() {
    let root = project("Player.lua", PLAYER);
    let mut scene = Scene::new("t");
    let ground = object(&mut scene, "Ground", Vec2::new(0.0, -3.0));
    scene.add(
        ground,
        BoxCollider2D {
            size: Vec2::new(40.0, 1.0),
            ..Default::default()
        },
    );
    let coin = object(&mut scene, "Coin", Vec2::new(0.0, -1.5));
    scene.add(
        coin,
        CircleCollider2D {
            common: Collider2DCommon {
                is_trigger: true,
                ..Default::default()
            },
            radius: 0.3,
        },
    );
    let player = object(&mut scene, "Player", Vec2::ZERO);
    scene.add(player, Rigidbody2D::default());
    scene.add(player, BoxCollider2D::default());
    scene.add(player, SpriteRenderer::default());
    scene.add(
        player,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Player.lua".into(),
                ..Default::default()
            }],
        },
    );
    let logs = run(&mut scene, root, 90);
    let has = |s: &str| logs.iter().any(|l| l == s);
    assert!(has("gravity true"), "{logs:?}");
    assert!(has("types Dynamic 1 1"), "{logs:?}");
    assert!(has("box 1 false true"), "{logs:?}");
    assert!(has("ray Coin 0.60 1 true"), "{logs:?}");
    assert!(has("miss true"), "{logs:?}");
    assert!(has("overlap Ground 0"), "{logs:?}");
    assert!(has("coin Coin true"), "{logs:?}");
    assert!(has("landed Ground true true Ground true"), "{logs:?}");
    assert!(!scene.contains(coin), "Destroy removes the trigger");
    let p = scene.world_position(player);
    assert!((p.y + 2.0).abs() < 0.05, "stands on the ground: {p}");
    assert!(p.x > 1.0, "moved right with its velocity: {p}");
    let sr = scene.world.get::<&SpriteRenderer>(player).unwrap();
    assert!(sr.flip_x && sr.order_in_layer == 3);
}

const MOVER: &str = r#"
local Mover = {}
function Mover:Start()
    Physics2D.gravity = Vector2.new(0, 0)
    self.rb = self.gameObject:GetComponent("Rigidbody2D")
    self.rb:AddForce(Vector2.new(0, 2), ForceMode2D.Impulse)
    Debug.Log("impulse", self.rb.velocity.y)
end
function Mover:Update()
    if Time.frameCount == 30 then
        self.rb:MovePosition(Vector2.new(10, 0))
    end
end
return Mover
"#;

#[test]
fn gravity_impulse_and_move_position() {
    let root = project("Mover.lua", MOVER);
    let mut scene = Scene::new("t");
    let e = object(&mut scene, "M", Vec2::ZERO);
    scene.add(e, Rigidbody2D::default());
    scene.add(e, CircleCollider2D::default());
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Mover.lua".into(),
                ..Default::default()
            }],
        },
    );
    let logs = run(&mut scene, root, 40);
    assert!(logs.iter().any(|l| l == "impulse 2"), "{logs:?}");
    let p = scene.world_position(e);
    assert!((p.x - 10.0).abs() < 1e-3, "{p}");
    // MovePosition placed it at y = 0 on frame 30; it kept its 2 m/s upwards since (no gravity).
    assert!(p.y > 0.25 && p.y < 0.45, "kept rising without gravity: {p}");
}

/// Rigidbody2D and Collider2D handles kept after `Destroy` raise a Lua error instead of
/// panicking.
#[test]
fn rigidbody2d_and_collider2d_of_a_destroyed_object_error_in_lua() {
    let src = r#"
local Keeper = { frames = 0 }
local function check(name, f)
    local ok, err = pcall(f)
    Debug.Log(name, ok, string.find(tostring(err), "destroyed", 1, true) ~= nil)
end
function Keeper:Start()
    local box = GameObject.Find("Box")
    self.rb = box:GetComponent("Rigidbody2D")
    self.col = box:GetComponent("BoxCollider2D")
    Destroy(box)
end
function Keeper:Update()
    self.frames = self.frames + 1
    if self.frames == 3 then
        local rb, col = self.rb, self.col
        check("position", function() return rb.position end)
        check("CompareTag", function() return col:CompareTag("Player") end)
    end
end
return Keeper
"#;
    let root = project("Keeper.lua", src);
    let mut scene = Scene::new("t");
    let b = object(&mut scene, "Box", Vec2::new(0.0, 5.0));
    scene.add(b, Rigidbody2D::default());
    scene.add(b, BoxCollider2D::default());
    let keeper = scene.spawn("Keeper", None);
    scene.add(
        keeper,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Keeper.lua".into(),
                ..Default::default()
            }],
        },
    );
    let logs = run(&mut scene, root, 5);
    for name in ["position", "CompareTag"] {
        assert!(
            logs.contains(&format!("{name} false true")),
            "{name}: {logs:?}"
        );
    }
}
