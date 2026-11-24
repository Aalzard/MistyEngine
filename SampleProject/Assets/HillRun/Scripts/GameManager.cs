using Misty;

/// Hill Run's game flow: the start menu (keyboard or gamepad through the Event System),
/// players joining through the Player Input Manager once Play is pressed (any key joins the
/// keyboard player, any button a gamepad player), split-screen cameras, scores, the pause menu
/// (Time.timeScale = 0) and the win screen.
public class GameManager : MonoBehaviour
{
    public static GameManager instance;

    [Header("Screens")]
    public GameObject menuPanel;
    public GameObject hud;
    public GameObject pausePanel;
    public GameObject winPanel;

    [Header("Buttons")]
    public Button playButton;
    public Button quitButton;
    public Button resumeButton;
    public Button menuButton;
    public Button againButton;

    [Header("Text")]
    public Text p1Score;
    public Text p2Score;
    public Text statusText;
    public Text winText;

    [Header("World")]
    public Camera lobbyCamera;
    public Transform spawn1;
    public Transform spawn2;

    [HideInInspector] public bool playing;
    [HideInInspector] public bool paused;
    [HideInInspector] public int pickupsLeft;

    readonly HillRunner[] runners = new HillRunner[2];

    void Awake() { instance = this; }

    void Start()
    {
        playButton.onClick.AddListener(StartGame);
        quitButton.onClick.AddListener(Application.Quit);
        resumeButton.onClick.AddListener(Resume);
        menuButton.onClick.AddListener(BackToMenu);
        againButton.onClick.AddListener(BackToMenu);
        Time.timeScale = 1f;
        menuPanel.SetActive(true);
        hud.SetActive(false);
        pausePanel.SetActive(false);
        winPanel.SetActive(false);
        EventSystem.current.SetSelectedGameObject(playButton.gameObject);
    }

    public void StartGame()
    {
        menuPanel.SetActive(false);
        hud.SetActive(true);
        playing = true;
        pickupsLeft = FindObjectsOfType<Pickup>().Length;
        EventSystem.current.SetSelectedGameObject(null);
        PlayerInputManager.instance.EnableJoining();
        Refresh();
    }

    // Sent by the Player Input Manager on this object.
    void OnPlayerJoined(PlayerInput player)
    {
        int i = player.playerIndex;
        var runner = player.GetComponent<HillRunner>();
        if (i < runners.Length) runners[i] = runner;
        player.transform.position = (i == 0 ? spawn1 : spawn2).position;
        player.gameObject.name = "Player " + (i + 1);
        runner.SetColor(i == 0 ? new Color(0.25f, 0.5f, 1f) : new Color(1f, 0.4f, 0.25f));
        lobbyCamera.enabled = false;
        if (PlayerInputManager.instance.playerCount >= runners.Length)
            PlayerInputManager.instance.DisableJoining();
        Refresh();
    }

    void OnPlayerLeft(PlayerInput player) { Refresh(); }

    public void TogglePause()
    {
        if (!playing) return;
        if (paused) Resume();
        else Pause();
    }

    public void Pause()
    {
        paused = true;
        Time.timeScale = 0f;
        pausePanel.SetActive(true);
        EventSystem.current.SetSelectedGameObject(resumeButton.gameObject);
    }

    public void Resume()
    {
        paused = false;
        Time.timeScale = 1f;
        pausePanel.SetActive(false);
        EventSystem.current.SetSelectedGameObject(null);
    }

    public void BackToMenu()
    {
        Time.timeScale = 1f;
        SceneManager.LoadScene("HillRun");
    }

    // A runner touched a pickup.
    public void Collected(HillRunner runner, int points)
    {
        if (!playing) return;
        runner.score += points;
        pickupsLeft--;
        Refresh();
        if (pickupsLeft <= 0) Win();
    }

    void Win()
    {
        playing = false;
        int a = Score(0), b = Score(1);
        winText.text = a == b ? $"Draw!  {a} - {b}" : $"Player {(a > b ? 1 : 2)} wins!  {a} - {b}";
        winPanel.SetActive(true);
        EventSystem.current.SetSelectedGameObject(againButton.gameObject);
    }

    int Score(int i) => runners[i] == null ? 0 : runners[i].score;

    void Refresh()
    {
        p1Score.text = runners[0] == null ? "P1: press a key" : "P1  " + Score(0);
        p2Score.text = runners[1] == null ? "P2: press a gamepad button" : "P2  " + Score(1);
        statusText.text = pickupsLeft + " pickups left";
    }
}
