use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use rv32i_sim::{Cpu, trace};

/// Run an RV32I program until it calls exit (ecall with a7 = 93).
///
/// The process exit code is the program's exit code; simulator errors exit with 2.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// RISC-V ELF32 executable, or a raw binary loaded at --mem-base.
    program: PathBuf,

    /// Print every retired instruction with its register/memory effects.
    #[arg(long)]
    trace: bool,

    /// Write a VCD waveform (clk, pc, instr, x1-x31) for GTKWave or Surfer.
    #[arg(long, value_name = "FILE")]
    vcd: Option<PathBuf>,

    /// Print the register file when the run ends.
    #[arg(long)]
    regs: bool,

    /// Give up after this many instructions.
    #[arg(long, default_value_t = 10_000_000)]
    max_steps: u64,

    /// RAM base address.
    #[arg(long, default_value = "0x80000000", value_parser = parse_u32)]
    mem_base: u32,

    /// RAM size in bytes.
    #[arg(long, default_value = "0x100000", value_parser = parse_u32)]
    mem_size: u32,
}

fn parse_u32(s: &str) -> Result<u32, String> {
    let s = s.replace('_', "");
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => s.parse(),
    }
    .map_err(|e| e.to_string())
}

const ABI_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

fn print_regs(cpu: &Cpu) {
    eprintln!("pc = {:#010x}", cpu.pc());
    for row in 0..8 {
        let line: Vec<String> = (0..4)
            .map(|col| {
                let r = row * 4 + col;
                let name = format!("x{r}/{}", ABI_NAMES[r]);
                format!("{name:<8} = {:#010x}", cpu.regs()[r])
            })
            .collect();
        eprintln!("{}", line.join("   "));
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(&args.program)
        .map_err(|e| format!("cannot read {}: {e}", args.program.display()))?;
    let mut cpu = rv32i_sim::load_program(&bytes, args.mem_base, args.mem_size as usize)?;

    let mut vcd = match &args.vcd {
        Some(path) => Some(trace::VcdWriter::new(
            BufWriter::new(File::create(path)?),
            &cpu,
        )?),
        None => None,
    };
    let mut out = BufWriter::new(io::stdout().lock());

    let result = cpu.run(args.max_steps, |cpu, commit| {
        if args.trace {
            // A closed pipe (e.g. `| head`) just ends the trace early.
            let _ = writeln!(out, "{}", trace::format_commit(cpu.instret(), commit));
        }
        if let Some(vcd) = vcd.as_mut() {
            vcd.record(commit);
        }
    });

    let _ = out.flush();
    if let Some(vcd) = vcd {
        vcd.finish()?;
    }

    match result {
        Ok(exit) => {
            if args.regs {
                print_regs(&cpu);
            }
            eprintln!("exit({}) after {} instructions", exit.code, exit.steps);
            Ok(ExitCode::from(exit.code.min(255) as u8))
        }
        Err(e) => {
            print_regs(&cpu);
            Err(e.into())
        }
    }
}
