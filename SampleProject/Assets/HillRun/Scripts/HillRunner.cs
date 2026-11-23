using Misty;

/// A Hill Run player (Runner.prefab, spawned by the Player Input Manager). Move walks (the
/// camera looks up the hill, so up is forward), Jump jumps while grounded, Pause opens the
/// pause menu: its Player Input sends OnJump / OnPause (Send Messages). The body turns towards
/// where it walks and its Animator hears the speed.
public class HillRunner : MonoBehaviour
{
    [Header("Movement")]
    public float walkSpeed = 4f;
    public float jumpHeight = 1.2f;
    public float gravity = -20f;
    public float turnSpeed = 720f;

    [Header("Parts")]
    public Transform body;
    public Animator animator;

    [HideInInspector] public int score;

    CharacterController controller;
    PlayerInput input;
    InputAction move;
    float verticalSpeed;
    bool jumpQueued;

    void Awake()
    {
        controller = GetComponent<CharacterController>();
        input = GetComponent<PlayerInput>();
    }

    void Start() { move = input.actions["Move"]; }

    void OnJump() { jumpQueued = true; }

    void OnPause()
    {
        if (GameManager.instance != null) GameManager.instance.TogglePause();
    }

    void Update()
    {
        float dt = Time.deltaTime;
        bool jump = jumpQueued;
        jumpQueued = false;
        if (dt <= 0f) return;
        bool canMove = GameManager.instance == null || GameManager.instance.playing;
        Vector2 stick = canMove ? move.ReadValue<Vector2>() : Vector2.zero;
        Vector3 direction = new Vector3(stick.x, 0f, stick.y);
        if (direction.magnitude > 1f) direction = direction.normalized;

        if (controller.isGrounded && verticalSpeed < 0f) verticalSpeed = -2f;
        if (jump && canMove && controller.isGrounded) verticalSpeed = Mathf.Sqrt(2f * jumpHeight * -gravity);
        verticalSpeed += gravity * dt;
        controller.Move((direction * walkSpeed + Vector3.up * verticalSpeed) * dt);

        if (direction.sqrMagnitude > 0.01f)
            body.rotation = Quaternion.RotateTowards(body.rotation, Quaternion.LookRotation(direction), turnSpeed * dt);
        Vector3 velocity = controller.velocity;
        velocity.y = 0f;
        animator.SetFloat("Speed", velocity.magnitude);
        animator.SetBool("Grounded", controller.isGrounded);
    }

    // Tints the shirt (the hips and chest boxes).
    public void SetColor(Color color)
    {
        foreach (var path in new[] { "Hips/Hips Mesh", "Hips/Spine/Chest/Chest Mesh" })
        {
            var part = body.Find(path);
            if (part != null) part.GetComponent<Renderer>().material.color = color;
        }
    }
}
