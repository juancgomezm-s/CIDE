// Native admission regression: production adapter + actual Host, no provider.
using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;

var directory = Path.GetFullPath(args[0]);
AssemblyLoadContext.Default.Resolving += (_, name) => File.Exists(Path.Combine(directory, name.Name + ".dll"))
    ? AssemblyLoadContext.Default.LoadFromAssemblyPath(Path.Combine(directory, name.Name + ".dll")) : null;
var host = AssemblyLoadContext.Default.LoadFromAssemblyPath(Path.Combine(directory, "OmniCore.Host.dll"));
var bridge = AssemblyLoadContext.Default.LoadFromAssemblyPath(Path.Combine(directory, "cide-omni-turn-v2.dll"));
var type = host.GetType("OmniCore.Host.OmniCliRuntime")!;
var runtime = type.GetMethod("Create", BindingFlags.Public | BindingFlags.Static)!
    .Invoke(null, [Path.GetFullPath(args[1]), null])!;
try
{
    var diagnostics = new List<string>();
    var prepared = bridge.GetType("EditorTurn")!.GetMethod("Prepare", BindingFlags.NonPublic | BindingFlags.Static)!
        .Invoke(null, [runtime, "Crea solamente ejemplo.p con DISPLAY 1.", (Action<string>)(s => diagnostics.Add(s)), CancellationToken.None]);
    var expected = args[2] == "allow";
    if ((prepared is not null) != expected) throw new Exception("Unexpected admission: " + string.Join(" | ", diagnostics));
    var client = type.GetMethod("Connect")!.Invoke(runtime, [CancellationToken.None])!;
    JsonDocument Query(string query)
    {
        var result = client.GetType().GetMethod("Query")!.Invoke(client, [query, CancellationToken.None])!;
        return JsonDocument.Parse((string)result.GetType().GetProperty("Json")!.GetValue(result)!);
    }
    using var defaults = Query("sessionSecurityDefault");
    if (defaults.RootElement.GetProperty("category").GetString() != "ObserveOnly"
        || defaults.RootElement.GetProperty("isConfigured").GetBoolean()) throw new Exception("Global defaults changed");
    using var model = Query("session.model.selection");
    if (expected)
    {
        using var security = Query("sessionSecurity");
        if (security.RootElement.GetProperty("category").GetString() != "ScopedCoder"
            || security.RootElement.GetProperty("fileMutationMode").GetString() != "PatchAndCreate") throw new Exception("Wrong edit scope");
        if (model.RootElement.GetProperty("selection").GetProperty("modelId").GetString() != "prueba"
            || model.RootElement.GetProperty("pendingConsent").ValueKind != JsonValueKind.Null) throw new Exception("Model binding was not completed");
    }
    else if (model.RootElement.GetProperty("activeRunId").ValueKind != JsonValueKind.Null)
        throw new Exception("Rejected route started a Run");
    if (Directory.EnumerateFileSystemEntries(args[1]).Any()) throw new Exception("Admission wrote workspace files");
    Console.WriteLine(JsonSerializer.Serialize(new { ok = true, admitted = expected, globalDefaultsUnchanged = true, providerInvoked = false }));
}
finally { ((IDisposable)runtime).Dispose(); }
