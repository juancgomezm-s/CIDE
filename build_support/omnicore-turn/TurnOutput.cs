using System.Text.Json;
using OmniCore.Client;

// The host emits one callback per trace or assistant response. Preserve that
// boundary, including multiline tool output, instead of guessing on each line.
internal static class TurnOutput
{
    internal static void Write(string original, Localization localization)
    {
        var text = localization.ResolveWire(original);
        if (Environment.GetEnvironmentVariable("CIDE_OMNICORE_WIRE") != "1")
        {
            Console.WriteLine(text);
            return;
        }
        var channel = text.StartsWith("[herramienta]", StringComparison.Ordinal)
            || text.StartsWith("[tool]", StringComparison.Ordinal) ? "tool"
            : text.StartsWith("[razonamiento]", StringComparison.Ordinal)
                || text.StartsWith("[reasoning]", StringComparison.Ordinal) ? "reasoning"
            : text.StartsWith("── ", StringComparison.Ordinal)
                || text.StartsWith("sesión ", StringComparison.Ordinal)
                || text.StartsWith("aviso:", StringComparison.Ordinal) ? "status" : "message";
        Console.WriteLine(JsonSerializer.Serialize(new { cideTurn = 1, channel, text }));
    }
}
