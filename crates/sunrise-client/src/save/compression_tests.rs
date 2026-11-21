use std::sync::Arc;

use sunrise_core::{
    nbt::{Limits, NamedTag, Tag},
    position::ChunkPos,
};
use sunrise_world::region::RegionFile;

use super::{
    Chunk, ChunkExtras, Encoding, LevelShape, Record, SaveDir,
    compression_pool::{CompressionPool, compress_records, compress_records_with_pool},
    write_region_records,
};

#[test]
fn a_failed_encode_never_strips_block_entity_edits_or_stops_the_batch() {
    use crate::containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields};
    use sunrise_core::nbt::Compound;

    let (registry, biomes) = super::loot_tests::fixture();
    let mut chunk = Chunk::empty(0, 0);
    let pos = (1, 64, 1);
    chunk.block_entities.push((
        pos,
        BlockEntity::Mod(Box::new(ModBlockEntity::new(
            "test:machine".into(),
            ModBlockEntityFields {
                nbt: Compound::from([("stored".into(), Tag::Int(7))]),
                ..Default::default()
            },
        ))),
    ));
    chunk.saved = Some(Arc::new(ChunkExtras {
        // TAG_End cannot be a named value. The unedited payload would encode.
        block_entity_save_edits: Some(Arc::new(
            [(pos, Compound::from([("cannot_encode".into(), Tag::End)]))].into(),
        )),
        ..Default::default()
    }));
    let encoding = Arc::new(Encoding {
        registry: Box::leak(Box::new(registry)),
        biomes: Arc::new(biomes),
        items: Default::default(),
        tables: None,
        shape: LevelShape::OVERWORLD,
    });
    let record = Record::Chunk(Arc::new(chunk), encoding, None);
    let healthy = Arc::new(NamedTag {
        name: "".into(),
        tag: Tag::compound([("xPos", Tag::Int(1)), ("zPos", Tag::Int(0))]),
    });
    let neighbor = Record::Tag(healthy.clone());
    let dir = tempfile::tempdir().unwrap();
    let save = SaveDir::new(dir.path().to_owned(), LevelShape::OVERWORLD, None);
    let failures = write_region_records(
        dir.path(),
        &save.regions,
        "region",
        &[((0, 0), &record), ((1, 0), &neighbor)],
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, (0, 0));
    assert!(failures[0].1.contains("End cannot be a named value"));
    let mut region = RegionFile::open(dir.path().join("region/r.0.0.mca")).unwrap();
    assert!(region.read(ChunkPos::new(0, 0)).unwrap().is_none());
    let compressed = region.read(ChunkPos::new(1, 0)).unwrap().unwrap();
    assert_eq!(compressed.nbt(Limits::saved()).unwrap(), *healthy);
}

fn sample_tag(i: i32) -> NamedTag {
    let payload: Vec<i64> = (0..((i * 131) % 1500).abs())
        .map(|v| (v as i64) * (i as i64 + 1))
        .collect();
    NamedTag {
        name: "".into(),
        tag: Tag::compound([
            ("xPos", Tag::Int(i)),
            ("zPos", Tag::Int(-i)),
            ("Data", Tag::LongArray(payload)),
        ]),
    }
}

#[test]
fn empty_and_single_chunk_compression() {
    assert!(compress_records(&[], 100).is_empty());

    let tag = Arc::new(sample_tag(42));
    let records = [((42, -42), Record::Tag(Arc::clone(&tag)))];
    let refs: Vec<_> = records.iter().map(|(k, r)| (*k, r)).collect();
    let compressed = compress_records(&refs, 555);
    assert_eq!(compressed.len(), 1);
    let record = compressed[0].as_ref().unwrap().as_ref().unwrap();
    assert_eq!(record.timestamp, 555);
    let loaded = record.nbt(Limits::saved()).unwrap();
    assert_eq!(loaded, *tag);
}

#[test]
fn parallel_compression_matches_single_threaded_fidelity() {
    let records: Vec<((i32, i32), Record)> = (0..64i32)
        .map(|i| {
            let record = if i % 5 == 2 {
                Record::Delete
            } else {
                Record::Tag(Arc::new(sample_tag(i)))
            };
            ((i, -i), record)
        })
        .collect();
    let refs: Vec<_> = records.iter().map(|(k, r)| (*k, r)).collect();
    let pool_compressed = compress_records(&refs, 9999);
    assert_eq!(pool_compressed.len(), records.len());

    let single_pool = CompressionPool::new(0);
    let single_compressed = compress_records_with_pool(&refs, 9999, &single_pool);

    for (i, (got_pool, got_single)) in pool_compressed
        .into_iter()
        .zip(single_compressed)
        .enumerate()
    {
        let rec_pool = got_pool.unwrap();
        let rec_single = got_single.unwrap();
        match (rec_pool, rec_single) {
            (None, None) => assert_eq!(i % 5, 2),
            (Some(p), Some(s)) => {
                assert_eq!(p.compression, s.compression);
                assert_eq!(p.timestamp, s.timestamp);
                assert_eq!(p.compressed, s.compressed);
                let tag_p = p.nbt(Limits::saved()).unwrap();
                let tag_s = s.nbt(Limits::saved()).unwrap();
                assert_eq!(tag_p, tag_s);
            }
            _ => panic!("mismatched option variant at index {i}"),
        }
    }
}

#[test]
fn custom_worker_pool_sizes_all_produce_identical_output() {
    let records: Vec<((i32, i32), Record)> = (0..27i32)
        .map(|i| ((i, i * 2), Record::Tag(Arc::new(sample_tag(i)))))
        .collect();
    let refs: Vec<_> = records.iter().map(|(k, r)| (*k, r)).collect();

    let pool_1 = CompressionPool::new(1);
    let pool_2 = CompressionPool::new(2);
    let pool_4 = CompressionPool::new(4);

    let res_1 = compress_records_with_pool(&refs, 12345, &pool_1);
    let res_2 = compress_records_with_pool(&refs, 12345, &pool_2);
    let res_4 = compress_records_with_pool(&refs, 12345, &pool_4);

    for idx in 0..records.len() {
        let r1 = res_1[idx].as_ref().unwrap().as_ref().unwrap();
        let r2 = res_2[idx].as_ref().unwrap().as_ref().unwrap();
        let r4 = res_4[idx].as_ref().unwrap().as_ref().unwrap();
        assert_eq!(r1.compressed, r2.compressed);
        assert_eq!(r1.compressed, r4.compressed);
        assert_eq!(r1.timestamp, r2.timestamp);
        assert_eq!(r1.timestamp, r4.timestamp);
    }
}

#[test]
fn compressed_chunks_written_to_region_file_reload_with_full_integrity() {
    let dir = std::env::temp_dir().join(format!("sunrise-chunk-save-pool-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let region_path = dir.join("r.0.0.mca");
    let mut region = RegionFile::open_writable(&region_path, true).unwrap();

    let count = 32i32;
    let records: Vec<((i32, i32), Record)> = (0..count)
        .map(|i| ((i, i), Record::Tag(Arc::new(sample_tag(i)))))
        .collect();
    let refs: Vec<_> = records.iter().map(|(k, r)| (*k, r)).collect();

    let compressed = compress_records(&refs, 77777);
    assert_eq!(compressed.len(), count as usize);

    for (idx, ((x, z), _)) in records.iter().enumerate() {
        let record = compressed[idx].as_ref().unwrap().as_ref().unwrap();
        region.write(ChunkPos::new(*x, *z), record).unwrap();
    }
    region.sync().unwrap();
    drop(region);

    let mut reopened = RegionFile::open(&region_path).unwrap();
    for ((x, z), record) in &records {
        let original_tag = record.tag().unwrap();
        let read_record = reopened
            .read(ChunkPos::new(*x, *z))
            .unwrap()
            .expect("chunk must exist");
        assert_eq!(read_record.timestamp, 77777);
        let read_tag = read_record.nbt(Limits::saved()).unwrap();
        assert_eq!(read_tag, *original_tag);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
