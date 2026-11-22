using System.Buffers.Binary;
using System.Text;
using OmniCore.Client;
using OmniCore.Host;

if (args is ["config"])
    return ConfigurationSnapshot.Print();

// The sibling runtime provides the effective workspace, model and permissions.
// The human request arrives over a private pipe rather than the process arguments.
if (args is not [("ask" or "edit"), var capacity] || !Console.IsInputRedirected
    || !int.TryParse(capacity, System.Globalization.NumberStyles.None,
        System.Globalization.CultureInfo.InvariantCulture, out var maximum) || maximum < 1)
    return 2;
var edit = args[0] == "edit";

Console.OutputEncoding = new UTF8Encoding(false);
var input = Console.OpenStandardInput();
var header = new byte[4];
string request;
try
{
    await input.ReadExactlyAsync(header);
    var length = BinaryPrimitives.ReadInt32LittleEndian(header);
    if (length < 1 || length > maximum)
        return 2;
    var bytes = new byte[length];
    await input.ReadExactlyAsync(bytes);
    request = new UTF8Encoding(false, true).GetString(bytes);
}
catch (Exception error) when (error is IOException or DecoderFallbackException)
{
    return 2;
}
if (string.IsNullOrWhiteSpace(request))
    return 2;

using var cancellation = new CancellationTokenSource();
_ = Task.Run(() => WatchCancellation(input, cancellation));
try
{
    using var runtime = OmniCliRuntime.Create(Directory.GetCurrentDirectory());
    var localization = new Localization("es");
    runtime.UiLocale = "es";
    if (edit) runtime.Localize = localization.Resolve;
    runtime.UseConsoleInput = false;
    Action<string> output = line => TurnOutput.Write(line, localization);
    if (edit) request = "CIDE editor integration: EDIT mode is an explicit request to change the current workspace, not merely show an example. Follow only the latest user request; prior questions are history. Discover only the filesystem tools needed, using tool.search maxResults at most 8. Read before patching; create a requested new file using the filesystem tool. Do not load unrelated skills or use desktop tools for a source edit. Respect effective permissions and approval requirements. Never claim a file was changed without a successful write.\n\n" + request;
    return edit
        ? Environment.GetEnvironmentVariable("CIDE_OMNICORE_EDIT_CONSENT") == "scope_v1"
            ? await EditorTurn.Run(runtime, request, output, cancellation.Token)
            : await runtime.ActAsync(request, output, cancellation.Token)
        // Localize known protocol messages after the host emits the full line.
        // The host's resolver can split arbitrary assistant text at a parenthesis.
        : await runtime.AskAsync(request, output, cancellation.Token);
}
catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
{
    TurnOutput.Write(edit ? "OmniCore canceló la edición." : "OmniCore canceló la consulta.", new Localization("es"));
    return 130;
}

catch (UnauthorizedAccessException)
{
    TurnOutput.Write("OmniCore no pudo acceder a su configuración o almacén de credenciales. Inicia CIDE con el acceso normal de tu usuario.", new Localization("es"));
    return 73;
}

static async Task WatchCancellation(Stream input, CancellationTokenSource cancellation)
{
    try
    {
        // Either a cancellation byte or the editor disconnecting stops the turn.
        await input.ReadAtLeastAsync(new byte[1], 1, false);
    }
    catch (IOException)
    {
    }
    try
    {
        await cancellation.CancelAsync();
    }
    catch (ObjectDisposedException)
    {
    }
}
