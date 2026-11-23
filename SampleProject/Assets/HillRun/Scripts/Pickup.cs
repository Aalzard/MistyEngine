using Misty;

/// A spinning, bobbing pickup (a trigger): the first runner to touch it scores.
public class Pickup : MonoBehaviour
{
    public int points = 1;
    public float spinSpeed = 120f;
    public float bobHeight = 0.15f;

    Vector3 start;

    void Start() { start = transform.position; }

    void Update()
    {
        transform.Rotate(0f, spinSpeed * Time.deltaTime, 0f);
        transform.position = start + Vector3.up * (Mathf.Sin(Time.time * 3f) * bobHeight);
    }

    void OnTriggerEnter(Collider other)
    {
        var runner = other.GetComponent<HillRunner>();
        if (runner == null || GameManager.instance == null || !GameManager.instance.playing) return;
        GameManager.instance.Collected(runner, points);
        gameObject.SetActive(false);
    }
}
