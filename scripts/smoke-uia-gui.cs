using System;
using System.Runtime.InteropServices;
using System.Windows.Automation;
using System.Windows.Automation.Text;

static class CideUiaSmoke
{
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, UIntPtr value,
        IntPtr parameter, uint flags, uint timeout, out UIntPtr result);

    static void Check(bool condition, string label)
    {
        if (!condition) throw new InvalidOperationException(label);
        Console.WriteLine(label);
    }

    static void Send(IntPtr window, uint message, uint value)
    {
        UIntPtr result;
        if (SendMessageTimeoutW(window, message, new UIntPtr(value), IntPtr.Zero, 2, 5000, out result) == IntPtr.Zero)
            throw new InvalidOperationException("El marco propio no respondió.");
    }

    [STAThread]
    public static int Main(string[] args)
    {
        Console.OutputEncoding = System.Text.Encoding.UTF8;
        try {
            var hwnd = new IntPtr(long.Parse(args[0]));
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            Check(pid == uint.Parse(args[1]), "marco del proceso de prueba");
            var element = AutomationElement.FromHandle(hwnd);
            var pattern = (TextPattern)element.GetCurrentPattern(TextPattern.Pattern);
            Check(element.Current.Name.Contains("uia-prueba.txt"), "nombre del documento");
            var original = "uno \U0001F642\r\ndos";
            Check(pattern.DocumentRange.GetText(-1) == original, "texto y CRLF");
            Check(pattern.DocumentRange.GetText(5) == "uno ", "UTF-16 no divide pares sustitutos");
            var line = pattern.DocumentRange.Clone();
            line.MoveEndpointByRange(TextPatternRangeEndpoint.End, line, TextPatternRangeEndpoint.Start);
            line.ExpandToEnclosingUnit(TextUnit.Line);
            Check(line.GetText(-1) == "uno \U0001F642\r\n", "lectura de una línea");
            var caret = pattern.GetSelection()[0];
            Check(caret.GetText(-1) == "", "cursor como rango vacío");
            var locations = caret.GetBoundingRectangles();
            Check(locations.Length == 1 && locations[0].Height > 0, "posición del cursor");
            var point = pattern.RangeFromPoint(locations[0].TopLeft);
            Check(point.Compare(caret), "rango desde coordenadas");
            caret.MoveEndpointByUnit(TextPatternRangeEndpoint.End, TextUnit.Character, 5);
            Check(caret.GetText(-1) == "uno \U0001F642", "movimiento de rango por caracteres");
            caret.Select();
            Check(pattern.GetSelection()[0].GetText(-1) == "uno \U0001F642", "selección lineal");
            Send(hwnd, 0x0007, 0); // WM_SETFOCUS only on the owned hidden window.
            Check((bool)element.GetCurrentPropertyValue(AutomationElement.HasKeyboardFocusProperty), "foco del editor");
            Send(hwnd, 0x0100, 0x24); // Home.
            Send(hwnd, 0x0102, (uint)'X');
            bool stale = false;
            try { line.GetText(-1); } catch (ElementNotAvailableException) { stale = true; }
            Check(stale, "rango obsoleto tras editar");
            Check(pattern.DocumentRange.GetText(-1) == "X" + original, "rango nuevo tras editar");
            return 0;
        } catch (Exception error) {
            Console.Error.WriteLine(error);
            return 1;
        }
    }
}
