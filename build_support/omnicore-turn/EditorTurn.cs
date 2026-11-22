using System.Text.Json;
using OmniCore.Host;
using OmniCore.Protocol;

// Only called after CIDE's explicit, version-pinned edit confirmation. Use the
// same typed human boundary as OmniCore's native UI; never change User defaults.
internal static class EditorTurn
{
    internal static async Task<int> Run(OmniCliRuntime runtime, string request,
        Action<string> output, CancellationToken token)
    {
        var prepared = Prepare(runtime, request, output, token);
        if (prepared is null) return 1;
        var (client, session) = prepared.Value;
        var host = new TuiTurnHost(runtime);
        var result = await host.ExecuteAsync(
            "Ejecuta ahora y únicamente la solicitud de edición anterior de CIDE en esta carpeta. Usa las herramientas de filesystem; confirma solo los cambios escritos. Respeta las políticas efectivas y las aprobaciones pendientes.",
            output, token);
        using var changes = JsonDocument.Parse(client.Query("changedFiles", token)?.Json ?? "null");
        var changed = changes.RootElement;
        if (changed.ValueKind == JsonValueKind.Object
            && changed.TryGetProperty("sessionId", out var owner) && owner.GetString() == session
            && changed.TryGetProperty("files", out var files) && files.ValueKind == JsonValueKind.Array)
        {
            var applied = files.EnumerateArray().Count(file => file.TryGetProperty("status", out var status)
                && status.GetString() is "A" or "M");
            output(applied == 0 ? "No se aplicaron cambios a archivos."
                : $"Cambios verificados por OmniCore: {applied} archivo(s) creado(s) o modificado(s).");
        }
        return result;
    }
    // Kept separate so native admission can be verified without contacting a model.
    internal static (IOmniClient Client, string Session)? Prepare(OmniCliRuntime runtime,
        string request, Action<string> output, CancellationToken token)
    {
        var client = runtime.Connect(token);
        if (client is not ITrustedUserActionClient human || !runtime.TryAcquireSessionWriter())
        {
            output("No se pudo abrir una sesión de edición de CIDE. Conserva tu solicitud e inténtalo de nuevo.");
            return null;
        }
        CommandAck Send(object command) => human.SendUserAction(
            WireEnvelope.Command(Guid.NewGuid().ToString("D"), JsonSerializer.Serialize(command)), token);
        if (Send(new { cmd = "conversation.new" }).Status != "ok")
        { output("No se pudo crear la conversación de edición."); return null; }
        using var security = JsonDocument.Parse(client.Query("sessionSecurity", token)?.Json ?? "null");
        var current = security.RootElement;
        if (current.ValueKind != JsonValueKind.Object)
        { output("No se pudo verificar el alcance de edición."); return null; }
        var session = current.GetProperty("sessionId").GetString();
        var revision = current.GetProperty("revision").GetInt64();
        if (Send(new { cmd = "session.security.set", sessionId = session,
                expectedRevision = revision, category = "ScopedCoder" }).Status != "ok")
        { output("OmniCore rechazó el alcance de edición confirmado. No se inició la consulta."); return null; }
        if (Send(new { cmd = "session.input", text = request, mode = "act", expectedPolicyRevision = revision + 1, expectedDefaultRevision = (string?)null }).Status != "ok")
        { output("No se pudo registrar la solicitud de edición."); return null; }
        return (client, session!);
    }

}
