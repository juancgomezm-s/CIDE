using System.Text.Json;
using OmniCore.Protocol;

// Bind only the model explicitly chosen in CIDE's version-pinned edit review.
// conversation.new starts with an empty routing snapshot in this runtime. Use
// native, CAS-checked model commands instead of accessing private Host APIs.
// This adapter initializes ONLY its just-created session, never a resumed one.
internal static class EditorModel
{
    internal static bool Bind(IOmniClient client, string session, Func<object, CommandAck> send,
        Action<string> output, CancellationToken token)
    {
        var id = Environment.GetEnvironmentVariable("OMNI_MODEL");
        if (string.IsNullOrWhiteSpace(id))
            return Fail("Elige un modelo en CIDE antes de enviar una edición.");
        using var catalog = Query("models.catalog");
        var matches = catalog.RootElement.GetProperty("models").EnumerateArray()
            .Where(model => model.GetProperty("modelId").GetString() == id).ToArray();
        if (matches.Length != 1) return Fail("El modelo elegido ya no está disponible. Vuelve a seleccionarlo.");
        var model = matches[0];
        var billing = model.GetProperty("billingMode").GetString();
        // Same non-metered baseline as native InitialSessionPolicy. A metered or
        // unknown route needs a separate native billing consent, never inference.
        if (billing is not ("Local" or "IncludedQuota" or "CreditBalance"))
            return Fail("Esta ruta requiere una autorización de facturación adicional en OmniCore. No se inició la edición.");
        using var before = Query("session.model.selection");
        var state = before.RootElement;
        if (!OwnIdle(state) || state.GetProperty("selectionRevision").GetInt64() != 0
            || state.GetProperty("routingPolicyRevision").GetInt64() != 1
            || state.GetProperty("pendingConsent").ValueKind != JsonValueKind.Null)
            return Fail("La sesión de edición cambió antes de fijar el modelo.");
        var ack = send(Command("session.model.select", state, null));
        if (ack.Status != "ok") return Fail("OmniCore rechazó la selección del modelo. No se inició la edición.");
        using var observed = Query("session.model.selection");
        state = observed.RootElement;
        if (!OwnIdle(state)) return Fail("La sesión cambió al seleccionar el modelo.");
        var pending = state.GetProperty("pendingConsent");
        if (pending.ValueKind != JsonValueKind.Null)
        {
            // The human confirmed this exact model in CIDE. Initialize that one
            // included/local route; never approve arbitrary runtime interactions.
            if (state.GetProperty("selectionRevision").GetInt64() != 0
                || state.GetProperty("routingPolicyRevision").GetInt64() != 1
                || new[] { "modelId", "providerId", "routeId", "billingMode" }
                    .Any(key => pending.GetProperty(key).GetString() != model.GetProperty(key).GetString()))
                return Fail("Cambió la ruta del modelo; vuelve a revisar la edición.");
            if (send(Command("session.model.route.respond", state,
                    pending.GetProperty("interactionId").GetString())).Status != "ok")
                return Fail("OmniCore no pudo fijar la ruta confirmada.");
        }
        using var accepted = Query("session.model.selection");
        state = accepted.RootElement;
        var selection = state.GetProperty("selection");
        return OwnIdle(state) && state.GetProperty("pendingConsent").ValueKind == JsonValueKind.Null
            && selection.ValueKind == JsonValueKind.Object
            && new[] { "modelId", "providerId", "routeId", "billingMode" }
                .All(key => selection.GetProperty(key).GetString() == model.GetProperty(key).GetString())
            || Fail("No se pudo verificar el modelo de la sesión de edición.");

        JsonDocument Query(string name) => JsonDocument.Parse(client.Query(name, token)?.Json ?? "null");
        bool OwnIdle(JsonElement current) => current.GetProperty("sessionId").GetString() == session
            && current.GetProperty("activeRunId").ValueKind == JsonValueKind.Null
            && current.GetProperty("sessionPolicyRevision").GetInt64() == 2
            && current.GetProperty("catalogRevision").GetString() == catalog.RootElement.GetProperty("catalogRevision").GetString();
        object Command(string name, JsonElement current, string? interaction) => new
        {
            cmd = name, modelId = id, expectedRunId = (string?)null,
            expectedSessionPolicyRevision = current.GetProperty("sessionPolicyRevision").GetInt64(),
            expectedRoutingPolicyRevision = current.GetProperty("routingPolicyRevision").GetInt64(),
            expectedSelectionRevision = current.GetProperty("selectionRevision").GetInt64(),
            catalogRevision = current.GetProperty("catalogRevision").GetString(),
            interactionId = interaction, optionId = interaction is null ? null : "allow_route"
        };
        bool Fail(string message) { output(message); return false; }
    }
}
