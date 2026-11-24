using Misty;

/// Highlights a menu button while it is selected (keyboard or gamepad navigation, or the
/// mouse): it grows a little and its label turns yellow.
public class MenuButton : MonoBehaviour, ISelectHandler, IDeselectHandler
{
    public Text label;
    public float selectedScale = 1.1f;

    public void OnSelect(BaseEventData eventData)
    {
        transform.localScale = Vector3.one * selectedScale;
        label.color = Color.yellow;
    }

    public void OnDeselect(BaseEventData eventData)
    {
        transform.localScale = Vector3.one;
        label.color = Color.white;
    }
}
