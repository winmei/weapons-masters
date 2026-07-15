namespace WeaponsMastersClient.Network;

public enum GameConnectionState
{
    Disconnected,
    Connecting,
    Authenticating,
    WaitingForWorld,
    InWorld,
    Reconnecting,
}
