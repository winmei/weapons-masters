#nullable enable
using System;

namespace WeaponsMastersClient.Game;

/// <summary>
/// Owns the small, explicit state machine for entering the authoritative world.
/// The first snapshot containing the local entity is the spawn confirmation.
/// </summary>
public sealed class WorldEntryService
{
    public WorldEntryState State { get; private set; } =
        WorldEntryState.WaitingForAuthentication;

    public bool IsReady => State == WorldEntryState.Ready;

    public event Action? WorldEntered;

    public void AuthenticationSent()
    {
        EnsureState(WorldEntryState.WaitingForAuthentication);
        State = WorldEntryState.WaitingForSnapshot;
    }

    public void ConfirmSpawn()
    {
        EnsureState(WorldEntryState.WaitingForSnapshot);
        State = WorldEntryState.Ready;
        WorldEntered?.Invoke();
    }

    private void EnsureState(WorldEntryState expectedState)
    {
        if (State != expectedState)
        {
            throw new InvalidOperationException(
                $"Expected world-entry state {expectedState}, current state is {State}."
            );
        }
    }
}
