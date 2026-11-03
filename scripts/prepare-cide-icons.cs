// Format/size conversion only: original PNGs and image_gen palette variant.
// No recolouring, new artwork, network access or package dependency here.
using System;
using System.IO;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
class PrepareCideIcons {
    static byte[] Png(string input,int size) {
        using(var source=Image.FromFile(input))
        using(var bitmap=new Bitmap(size,size,PixelFormat.Format32bppArgb)) {
            using(var graphics=Graphics.FromImage(bitmap)) {
                graphics.Clear(Color.Transparent);
                graphics.CompositingMode=CompositingMode.SourceCopy;
                graphics.CompositingQuality=CompositingQuality.HighQuality;
                graphics.InterpolationMode=InterpolationMode.HighQualityBicubic;
                graphics.PixelOffsetMode=PixelOffsetMode.HighQuality;
                graphics.DrawImage(source,new Rectangle(0,0,size,size));
            }
            using(var stream=new MemoryStream()){bitmap.Save(stream,ImageFormat.Png);return stream.ToArray();}
        }
    }
    static void Ico(string path,byte[][] frames,int[] sizes) {
        using(var stream=new FileStream(path,FileMode.CreateNew,FileAccess.Write))
        using(var writer=new BinaryWriter(stream)) {
            writer.Write((ushort)0);writer.Write((ushort)1);writer.Write((ushort)frames.Length);
            int offset=6+16*frames.Length;
            for(int i=0;i<frames.Length;i++) {
                writer.Write((byte)(sizes[i]==256?0:sizes[i]));writer.Write((byte)(sizes[i]==256?0:sizes[i]));writer.Write((byte)0);writer.Write((byte)0);
                writer.Write((ushort)1);writer.Write((ushort)32);writer.Write(frames[i].Length);writer.Write(offset);offset+=frames[i].Length;
            }
            foreach(var frame in frames)writer.Write(frame);
        }
    }
    static int Main(string[] args) {
        if(args.Length!=1)return 2;
        string root=Path.GetFullPath(args[0]);int[] sizes={16,24,32,48,64,128,256};
        var original=new byte[sizes.Length][];var orange=new byte[sizes.Length][];
        for(int i=0;i<sizes.Length;i++) {
            original[i]=File.ReadAllBytes(Path.Combine(root,"cide-tui-"+sizes[i]+".png"));
            orange[i]=Png(Path.Combine(root,"cide-gui-orange-source.png"),sizes[i]);
            using(var file=new FileStream(Path.Combine(root,"cide-gui-"+sizes[i]+".png"),FileMode.CreateNew))file.Write(orange[i],0,orange[i].Length);
        }
        Ico(Path.Combine(root,"cide-tui.ico"),original,sizes);Ico(Path.Combine(root,"cide-gui.ico"),orange,sizes);
        Console.WriteLine("CIDE green TUI and orange GUI multi-resolution ICO assets prepared");return 0;
    }
}
