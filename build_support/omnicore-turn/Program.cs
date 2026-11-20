using System.Buffers.Binary;
using System.Text;
using OmniCore.Client;
using OmniCore.Host;

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
using var runtime = OmniCliRuntime.Create(Directory.GetCurrentDirectory());
var localization = new Localization("es");
runtime.UiLocale = "es";
if (edit)
    runtime.Localize = localization.Resolve;
runtime.UseConsoleInput = false;
try
{
    return edit
        ? await runtime.ActAsync(request, Console.WriteLine, cancellation.Token)
        // Localize known protocol messages after the host emits the full line.
        // The host's resolver can split arbitrary assistant text at a parenthesis.
        : await runtime.AskAsync(request, line => Console.WriteLine(localization.ResolveWire(line)), cancellation.Token);
}
catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
{
    Console.WriteLine(edit ? "OmniCore canceló la edición." : "OmniCore canceló la consulta.");
    return 130;
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
