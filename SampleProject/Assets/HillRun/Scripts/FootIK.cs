using Misty;

/// Keeps the runner's feet on the ground (slopes, steps) in the Animator's IK pass: each
/// foot's animated position goes onto the ground a ray finds straight below it, and the head
/// looks a little ahead. The Runner controller's Base Layer has IK Pass on.
public class FootIK : MonoBehaviour
{
    public bool ikActive = true;
    [Tooltip("Height of the ankle above the sole")]
    public float footOffset = 0.03f;
    public float rayLength = 1.2f;
    public float lookAhead = 3f;

    Animator animator;
    CharacterController controller;

    void Start()
    {
        animator = GetComponent<Animator>();
        controller = GetComponentInParent<CharacterController>();
    }

    void OnAnimatorIK(int layerIndex)
    {
        bool grounded = ikActive && controller != null && controller.isGrounded;
        Foot(AvatarIKGoal.LeftFoot, grounded);
        Foot(AvatarIKGoal.RightFoot, grounded);
        Vector3 head = animator.GetBoneTransform(HumanBodyBones.Head).position;
        animator.SetLookAtPosition(head + transform.forward * lookAhead);
        animator.SetLookAtWeight(ikActive ? 0.6f : 0f, 0f, 1f, 0f, 0.5f);
    }

    void Foot(AvatarIKGoal goal, bool grounded)
    {
        float weight = 0f;
        Vector3 foot = animator.GetIKPosition(goal);
        if (grounded && Physics.Raycast(foot + Vector3.up * (rayLength * 0.5f), Vector3.down, out RaycastHit hit, rayLength))
        {
            animator.SetIKPosition(goal, hit.point + Vector3.up * footOffset);
            animator.SetIKRotation(goal, Quaternion.FromToRotation(Vector3.up, hit.normal) * transform.rotation);
            weight = 1f;
        }
        animator.SetIKPositionWeight(goal, weight);
        animator.SetIKRotationWeight(goal, weight);
    }
}
