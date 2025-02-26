

use std::sync::Arc;
use tokio;
use tokio::fs::read_to_string;
use anyhow;
use futures::future;
use tabled::builder::Builder;
use tabled::settings::{ Alignment, Style};
use tabled::settings::object::{Columns, Rows};
use nvml_wrapper::{enums::device::UsedGpuMemory, Nvml};
use regex;


struct ShowInfo{
    id:u32,
    name:String,
    bus_id:String,
    gpumem:String,
    pid:u32,
    pstatus:String,
    pmem:String,
    pgpumem:String,
    pcmdline:String
}

#[tokio::main]
async fn main()->anyhow::Result<()> {
    
    let nvml = Arc::new(Nvml::init()?);
    let (tx,mut rx) = tokio::sync::mpsc::channel(16);

    let mut tasks = Vec::new();

    for i in 0..nvml.device_count()?{
        let tx_clone  = tx.clone();
        let nvml = nvml.clone();
        let task:tokio::task::JoinHandle<Result<(), anyhow::Error>>  = tokio::spawn(async move{
            let device = nvml.device_by_index(i)?;
            let id  = i;
            let name = device.name()?;
            let bus_id = device.pci_info()?.bus_id;
            let (gpufree,gpuused,gputotal) = if let Ok(mi)= device.memory_info(){
                    (mi.free,mi.used,mi.total)
            }else{
                (0,0,0)
            };
            let gpumem  = format!("free {}:{}/{}",humansize::format_size(gpufree, humansize::BINARY),humansize::format_size(gpuused, humansize::BINARY),humansize::format_size(gputotal, humansize::BINARY));
            let processes = device.running_compute_processes()?
                            .into_iter()
                            .chain(device.running_graphics_processes()?.into_iter())
                            .collect::<Vec<_>>();

            for pinfo in processes{
                let pid  = pinfo.pid;
                let pgpumem = humansize::format_size(if let UsedGpuMemory::Used(byte) = pinfo.used_gpu_memory{
                    byte
                }else{
                    0
                },humansize::BINARY);
                let pcmdline = read_to_string(format!("/proc/{}/cmdline",pid)).await.unwrap_or("".to_owned()).replace('\0', " ");
                let status_string =  read_to_string(format!("/proc/{}/status",pid)).await.unwrap_or("".to_owned());
                let reg  = regex::Regex::new(r"State:.+\(([a-zA-Z]+)\)")?;
                let pstatus = reg.captures(status_string.as_str()).map_or("".to_owned(),|cap|cap[1].to_owned());
                
                let reg  = regex::Regex::new(r"VmRSS:\s+([0-9]+\s[a-zA-Z]+)")?;
                let pmem = reg.captures(status_string.as_str()).map_or("".to_owned(),|cap|cap[1].to_owned());
                
                

                tx_clone.send(
                    ShowInfo{
                        id:id,
                        name:name.to_owned(),
                        bus_id:bus_id.to_owned(),
                        gpumem:gpumem.to_owned(),
                        pid:pid,
                        pstatus:pstatus,
                        pmem:pmem,
                        pgpumem:pgpumem,
                        pcmdline:pcmdline
                    }
                ).await?;
            }
            Ok(())
        });
        tasks.push(task);
    }

    future::join_all(tasks).await;
    drop(tx);

   let mut builder  = Builder::default();
   builder.push_record(vec!["index","name","bus_id","gpumem","pid","pstatus","pmem","pgpumem","pcmdline"]);
    while let Some(info) = rx.recv().await{
        builder.push_record(vec![
            info.id.to_string(),
            info.name,
            info.bus_id,
            info.gpumem,
            info.pid.to_string(),
            info.pstatus,
            info.pmem,
            info.pgpumem,
            info.pcmdline
        ]);
    }
    println!("{}",builder.build()
    .with(Style::psql())
    .with(Alignment::center())
    .modify(Columns::single(1),Alignment::left())
    .modify(Columns::last(), Alignment::left())
    .modify(Rows::first(), Alignment::center())
    );
    Ok(())
}
