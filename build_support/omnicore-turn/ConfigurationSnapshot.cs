using System.Text;
using System.Text.Json;
using OmniCore.Host;

// Review metadata only. Credentials, authRef values and raw YAML never leave
// OmniCore's loader. This mode neither opens a Run nor rewrites user settings.
internal static class ConfigurationSnapshot
{
    internal static int Print()
    {
        Console.OutputEncoding = new UTF8Encoding(false);
        using var buffer = new MemoryStream();
        using (var output = new Utf8JsonWriter(buffer))
        {
            output.WriteStartObject();
            output.WriteNumber("schemaVersion", 1);
            try
            {
                var root = Environment.GetEnvironmentVariable("OMNICORE_CONFIG_DIR")
                    ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "OmniCore");
                if (!Path.IsPathFullyQualified(root)) throw new IOException();
                string? Read(string name)
                {
                    var path = Path.Combine(root, name);
                    if (!File.Exists(path)) return null;
                    var info = new FileInfo(path);
                    var length = info.Length; var stamp = info.LastWriteTimeUtc;
                    if (length > 512 * 1024) throw new IOException();
                    using var file = File.OpenRead(path);
                    using var bounded = new MemoryStream();
                    var chunk = new byte[8192];
                    int count;
                    while ((count = file.Read(chunk)) != 0)
                    {
                        if (bounded.Length + count > 512 * 1024) throw new IOException();
                        bounded.Write(chunk, 0, count);
                    }
                    var bytes = bounded.ToArray();
                    info.Refresh();
                    if (bytes.Length != length || info.Length != length || info.LastWriteTimeUtc != stamp) throw new IOException();
                    return new UTF8Encoding(false, true).GetString(bytes).TrimStart('\uFEFF');
                }
                var loaded = new ConfigLoader().Load(Read("providers.yaml"), Read("models.yaml"), Read("settings.yaml"));
                if ((loaded.Providers?.Providers?.Count ?? 0) > 128 || (loaded.Models?.Models?.Count ?? 0) > 1024) throw new IOException();
                output.WriteString("configDirectory", root);
                output.WriteString("defaultModel", loaded.Settings?.DefaultModel);
                output.WriteNumber("modelCount", loaded.Models?.Models?.Count ?? 0);
                output.WriteBoolean("legacyAuthentication", loaded.Providers?.Providers?.Values.Any(p => p.Auth is not null) == true);
                output.WriteStartArray("providers");
                foreach (var (id, provider) in (loaded.Providers?.Providers ?? new Dictionary<string, ProviderFileYaml>()).OrderBy(p => p.Key))
                {
                    output.WriteStartObject();
                    output.WriteString("id", id);
                    var chatGpt = string.Equals(provider.Profile, "codex", StringComparison.OrdinalIgnoreCase)
                        && Uri.TryCreate(provider.BaseUrl, UriKind.Absolute, out var accountUri)
                        && string.Equals(accountUri.Host, "chatgpt.com", StringComparison.OrdinalIgnoreCase);
                    output.WriteString("kind", chatGpt ? "ChatGPT · OpenAI Responses" : provider.Kind ?? provider.Family ?? "No declarado");
                    var address = "No declarada";
                    if (Uri.TryCreate(provider.BaseUrl, UriKind.Absolute, out var uri) && uri.Scheme is "http" or "https")
                        address = new UriBuilder(uri) { UserName = "", Password = "", Query = "", Fragment = "" }.Uri.AbsoluteUri;
                    output.WriteString("address", address);
                    output.WriteString("authentication", chatGpt ? "Cuenta ChatGPT"
                        : provider.OAuth is not null ? "Cuenta OAuth"
                        : provider.AuthRef is not null ? "Clave mediante referencia"
                        : provider.Auth is { IsNone: false } ? "Autenticación antigua"
                        : "Sin autenticación declarada");
                    output.WriteString("billing", provider.BillingMode ?? "No declarado");
                    output.WriteEndObject();
                }
                output.WriteEndArray();
            }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException or ConfigValidationException or DecoderFallbackException)
            {
                // Do not return diagnostics containing the original secret-bearing YAML.
                output.WriteString("error", error is ConfigValidationException
                    ? "La configuración de OmniCore no es válida. Revisa providers.yaml, models.yaml y settings.yaml."
                    : "No se pudo leer la configuración de OmniCore con el acceso actual.");
            }
            output.WriteEndObject();
        }
        Console.WriteLine(Encoding.UTF8.GetString(buffer.ToArray()));
        return 0;
    }
}
