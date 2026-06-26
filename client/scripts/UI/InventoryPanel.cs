using System.Collections.Generic;
using Godot;
using WeaponsMastersClient.Autoload;
using Wm;

namespace WeaponsMastersClient.UI;

/// <summary>
/// Inventory panel — toggle with the "open_inventory" action (I key).
///
/// Data flow:
///   1. Scene load  → PopulateFromSession reads Session.Character.Inventory.
///   2. Runtime     → PacketHandler calls OnLootDrop when a LootDrop event
///                    arrives in a WorldSnapshot for the local entity.
///
/// The panel never contacts the server directly; it mirrors the authoritative
/// state that the server already wrote to PostgreSQL via NATS.
/// </summary>
public partial class InventoryPanel : Panel
{
    private Label?    _titleLabel;
    private ItemList? _itemList;

    // Canonical in-memory inventory: slot index → (item_name, quantity).
    // Matches the player_inventory table schema (slot is the PK alongside character_id).
    private readonly Dictionary<int, (string Name, uint Quantity)> _slots = new();

    public override void _Ready()
    {
        _titleLabel = GetNodeOrNull<Label>("VBox/Title");
        _itemList   = GetNodeOrNull<ItemList>("VBox/Items");

        Visible = false; // hidden until player presses I

        PopulateFromSession();
    }

    /// <summary>
    /// Toggle visibility when the player presses the inventory action.
    /// Uses _UnhandledInput so UI-consumed clicks don't reach this node.
    /// </summary>
    public override void _UnhandledInput(InputEvent @event)
    {
        if (@event.IsActionJustPressed("open_inventory"))
        {
            Visible = !Visible;
            GetViewport().SetInputAsHandled();
        }
    }

    /// <summary>
    /// Called by PacketHandler when a LootDrop event arrives for the local entity.
    /// Accumulates quantity if the same item is already in the slot; replaces otherwise.
    /// </summary>
    public void OnLootDrop(int slot, string itemName, uint quantity)
    {
        if (_slots.TryGetValue(slot, out var existing) && existing.Name == itemName)
        {
            _slots[slot] = (itemName, existing.Quantity + quantity);
        }
        else
        {
            _slots[slot] = (itemName, quantity);
        }
        RefreshItemList();
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    private void PopulateFromSession()
    {
        var character = Session.Instance?.Character;
        if (character is null) return;

        foreach (var invSlot in character.Inventory)
        {
            if (invSlot.Item is null) continue;
            _slots[invSlot.Slot] = (invSlot.Item.ItemName, invSlot.Item.Quantity);
        }

        if (_titleLabel is not null)
        {
            _titleLabel.Text = $"[ Inventário — {character.Name} ]";
        }

        RefreshItemList();
    }

    private void RefreshItemList()
    {
        if (_itemList is null) return;

        _itemList.Clear();

        if (_slots.Count == 0)
        {
            _itemList.AddItem("— inventário vazio —");
            return;
        }

        // Display slots sorted by index for a stable, predictable order.
        var sortedKeys = new List<int>(_slots.Keys);
        sortedKeys.Sort();

        foreach (var slot in sortedKeys)
        {
            var (name, qty) = _slots[slot];
            _itemList.AddItem($"[{slot:D2}]  {name}  ×{qty}");
        }
    }
}
