using System;
using System.Diagnostics;
using System.IO;

public class SupervisorBootstrapper {
    private static String logPath = Path.Combine(Environment.GetEnvironmentVariable("DOJ_STUDIO_ENTER_ROOT"), "doj\\sup\\default\\out.log");

    public static void Run(bool isAnsiSupported) {
        var proc = new Process();
        proc.StartInfo.UseShellExecute = false;
        proc.StartInfo.CreateNoWindow = true;
        proc.StartInfo.RedirectStandardOutput = true;
        proc.StartInfo.RedirectStandardError = true;
        proc.StartInfo.FileName = "doj.exe";
        if (isAnsiSupported) {
            proc.StartInfo.Arguments = "sup run";
        }
        else {
            proc.StartInfo.Arguments = "sup run --no-color";
            proc.StartInfo.EnvironmentVariables["DOJ_NOCOLORING"] = "1";
        }
        proc.OutputDataReceived += new DataReceivedEventHandler(SupOutputHandler);
        proc.ErrorDataReceived += new DataReceivedEventHandler(SupOutputHandler);
        proc.Start();
        proc.BeginErrorReadLine();
        proc.BeginOutputReadLine();
    }

    private static void SupOutputHandler(object sender, DataReceivedEventArgs e) {
        if (!String.IsNullOrEmpty(e.Data)) {
            using (FileStream fs = new FileStream(logPath, FileMode.Append, FileAccess.Write, FileShare.ReadWrite)) {
                using (StreamWriter sw = new StreamWriter(fs, System.Text.Encoding.UTF8)) {
                    sw.WriteLine(e.Data);
                }
            }
        }
    }
}
