pub const CRYPTO_LLVM_PRELUDE: &str = r#"
; --- nialang crypto builtins ---
@nialang.crypto.sha256.k = private unnamed_addr constant [64 x i32] [
  i32 1116352408, i32 1899447441, i32 -1245643825, i32 -373957723,
  i32 961987163, i32 1508970993, i32 -1841331548, i32 -1424204075,
  i32 -670586216, i32 310598401, i32 607225278, i32 1426881987,
  i32 1925078388, i32 -2132889090, i32 -1680079193, i32 -1046744716,
  i32 -459576895, i32 -272742522, i32 264347078, i32 604807628,
  i32 770255983, i32 1249150122, i32 1555081692, i32 1996064986,
  i32 -1740746414, i32 -1473132947, i32 -1341970488, i32 -1084653625,
  i32 -958395405, i32 -710438585, i32 113926993, i32 338241895,
  i32 666307205, i32 773529912, i32 1294757372, i32 1396182291,
  i32 1695183700, i32 1986661051, i32 -2117940946, i32 -1838011259,
  i32 -1564481375, i32 -1474664885, i32 -1035236496, i32 -949202525,
  i32 -778901479, i32 -694614492, i32 -200395387, i32 275423344,
  i32 430227734, i32 506948616, i32 659060556, i32 883997877,
  i32 958139571, i32 1322822218, i32 1537002063, i32 1747873779,
  i32 1955562222, i32 2024104815, i32 -2067236844, i32 -1933114872,
  i32 -1866530822, i32 -1538233109, i32 -1090935817, i32 -965641998
], align 4

define i32 @nialang.crypto.rotr32(i32 %x, i32 %n) {
entry:
  %r = lshr i32 %x, %n
  %sub = sub i32 32, %n
  %l = shl i32 %x, %sub
  %out = or i32 %r, %l
  ret i32 %out
}

define void @nialang.crypto.copy_bytes(ptr %dst, ptr %src, i64 %len) {
entry:
  br label %loop

loop:
  %i = phi i64 [ 0, %entry ], [ %next, %body ]
  %done = icmp uge i64 %i, %len
  br i1 %done, label %exit, label %body

body:
  %sp = getelementptr i8, ptr %src, i64 %i
  %dp = getelementptr i8, ptr %dst, i64 %i
  %v = load i8, ptr %sp, align 1
  store i8 %v, ptr %dp, align 1
  %next = add i64 %i, 1
  br label %loop

exit:
  ret void
}

define void @nialang.crypto.zero_bytes(ptr %dst, i64 %len) {
entry:
  br label %loop

loop:
  %i = phi i64 [ 0, %entry ], [ %next, %body ]
  %done = icmp uge i64 %i, %len
  br i1 %done, label %exit, label %body

body:
  %dp = getelementptr i8, ptr %dst, i64 %i
  store i8 0, ptr %dp, align 1
  %next = add i64 %i, 1
  br label %loop

exit:
  ret void
}

define i32 @nialang.crypto.load_be32(ptr %src, i64 %word_index) {
entry:
  %base = mul i64 %word_index, 4
  %p0 = getelementptr i8, ptr %src, i64 %base
  %b0 = load i8, ptr %p0, align 1
  %i1 = add i64 %base, 1
  %p1 = getelementptr i8, ptr %src, i64 %i1
  %b1 = load i8, ptr %p1, align 1
  %i2 = add i64 %base, 2
  %p2 = getelementptr i8, ptr %src, i64 %i2
  %b2 = load i8, ptr %p2, align 1
  %i3 = add i64 %base, 3
  %p3 = getelementptr i8, ptr %src, i64 %i3
  %b3 = load i8, ptr %p3, align 1
  %z0 = zext i8 %b0 to i32
  %z1 = zext i8 %b1 to i32
  %z2 = zext i8 %b2 to i32
  %z3 = zext i8 %b3 to i32
  %s0 = shl i32 %z0, 24
  %s1 = shl i32 %z1, 16
  %s2 = shl i32 %z2, 8
  %o0 = or i32 %s0, %s1
  %o1 = or i32 %o0, %s2
  %out = or i32 %o1, %z3
  ret i32 %out
}

define void @nialang.crypto.store_be32(ptr %dst, i64 %word_index, i32 %value) {
entry:
  %base = mul i64 %word_index, 4
  %b0s = lshr i32 %value, 24
  %b0 = trunc i32 %b0s to i8
  %p0 = getelementptr i8, ptr %dst, i64 %base
  store i8 %b0, ptr %p0, align 1
  %b1s = lshr i32 %value, 16
  %b1 = trunc i32 %b1s to i8
  %i1 = add i64 %base, 1
  %p1 = getelementptr i8, ptr %dst, i64 %i1
  store i8 %b1, ptr %p1, align 1
  %b2s = lshr i32 %value, 8
  %b2 = trunc i32 %b2s to i8
  %i2 = add i64 %base, 2
  %p2 = getelementptr i8, ptr %dst, i64 %i2
  store i8 %b2, ptr %p2, align 1
  %b3 = trunc i32 %value to i8
  %i3 = add i64 %base, 3
  %p3 = getelementptr i8, ptr %dst, i64 %i3
  store i8 %b3, ptr %p3, align 1
  ret void
}

define void @nialang.crypto.sha256_compress(ptr %state, ptr %block) {
entry:
  %w = alloca [64 x i32], align 4
  br label %w_init_loop

w_init_loop:
  %wi = phi i64 [ 0, %entry ], [ %wi_next, %w_init_body ]
  %wi_done = icmp uge i64 %wi, 16
  br i1 %wi_done, label %w_extend_loop, label %w_init_body

w_init_body:
  %word = call i32 @nialang.crypto.load_be32(ptr %block, i64 %wi)
  %wptr = getelementptr [64 x i32], ptr %w, i64 0, i64 %wi
  store i32 %word, ptr %wptr, align 4
  %wi_next = add i64 %wi, 1
  br label %w_init_loop

w_extend_loop:
  %we = phi i64 [ 16, %w_init_loop ], [ %we_next, %w_extend_body ]
  %we_done = icmp uge i64 %we, 64
  br i1 %we_done, label %round_entry, label %w_extend_body

w_extend_body:
  %im15 = sub i64 %we, 15
  %wim15p = getelementptr [64 x i32], ptr %w, i64 0, i64 %im15
  %wim15 = load i32, ptr %wim15p, align 4
  %r7 = call i32 @nialang.crypto.rotr32(i32 %wim15, i32 7)
  %r18 = call i32 @nialang.crypto.rotr32(i32 %wim15, i32 18)
  %sh3 = lshr i32 %wim15, 3
  %s0a = xor i32 %r7, %r18
  %s0 = xor i32 %s0a, %sh3
  %im2 = sub i64 %we, 2
  %wim2p = getelementptr [64 x i32], ptr %w, i64 0, i64 %im2
  %wim2 = load i32, ptr %wim2p, align 4
  %r17 = call i32 @nialang.crypto.rotr32(i32 %wim2, i32 17)
  %r19 = call i32 @nialang.crypto.rotr32(i32 %wim2, i32 19)
  %sh10 = lshr i32 %wim2, 10
  %s1a = xor i32 %r17, %r19
  %s1 = xor i32 %s1a, %sh10
  %im16 = sub i64 %we, 16
  %wim16p = getelementptr [64 x i32], ptr %w, i64 0, i64 %im16
  %wim16 = load i32, ptr %wim16p, align 4
  %im7 = sub i64 %we, 7
  %wim7p = getelementptr [64 x i32], ptr %w, i64 0, i64 %im7
  %wim7 = load i32, ptr %wim7p, align 4
  %sum0 = add i32 %wim16, %s0
  %sum1 = add i32 %sum0, %wim7
  %sum2 = add i32 %sum1, %s1
  %weptr = getelementptr [64 x i32], ptr %w, i64 0, i64 %we
  store i32 %sum2, ptr %weptr, align 4
  %we_next = add i64 %we, 1
  br label %w_extend_loop

round_entry:
  %s0p = getelementptr [8 x i32], ptr %state, i64 0, i64 0
  %s1p = getelementptr [8 x i32], ptr %state, i64 0, i64 1
  %s2p = getelementptr [8 x i32], ptr %state, i64 0, i64 2
  %s3p = getelementptr [8 x i32], ptr %state, i64 0, i64 3
  %s4p = getelementptr [8 x i32], ptr %state, i64 0, i64 4
  %s5p = getelementptr [8 x i32], ptr %state, i64 0, i64 5
  %s6p = getelementptr [8 x i32], ptr %state, i64 0, i64 6
  %s7p = getelementptr [8 x i32], ptr %state, i64 0, i64 7
  %a0 = load i32, ptr %s0p, align 4
  %b0 = load i32, ptr %s1p, align 4
  %c0 = load i32, ptr %s2p, align 4
  %d0 = load i32, ptr %s3p, align 4
  %e0 = load i32, ptr %s4p, align 4
  %f0 = load i32, ptr %s5p, align 4
  %g0 = load i32, ptr %s6p, align 4
  %h0 = load i32, ptr %s7p, align 4
  br label %round_loop

round_loop:
  %i = phi i64 [ 0, %round_entry ], [ %i_next, %round_body ]
  %a = phi i32 [ %a0, %round_entry ], [ %new_a, %round_body ]
  %b = phi i32 [ %b0, %round_entry ], [ %a, %round_body ]
  %c = phi i32 [ %c0, %round_entry ], [ %b, %round_body ]
  %d = phi i32 [ %d0, %round_entry ], [ %c, %round_body ]
  %e = phi i32 [ %e0, %round_entry ], [ %new_e, %round_body ]
  %f = phi i32 [ %f0, %round_entry ], [ %e, %round_body ]
  %g = phi i32 [ %g0, %round_entry ], [ %f, %round_body ]
  %h = phi i32 [ %h0, %round_entry ], [ %g, %round_body ]
  %done = icmp uge i64 %i, 64
  br i1 %done, label %round_exit, label %round_body

round_body:
  %er6 = call i32 @nialang.crypto.rotr32(i32 %e, i32 6)
  %er11 = call i32 @nialang.crypto.rotr32(i32 %e, i32 11)
  %er25 = call i32 @nialang.crypto.rotr32(i32 %e, i32 25)
  %s1x = xor i32 %er6, %er11
  %bs1 = xor i32 %s1x, %er25
  %ef = and i32 %e, %f
  %ne = xor i32 %e, -1
  %neg = and i32 %ne, %g
  %ch = xor i32 %ef, %neg
  %kptr = getelementptr [64 x i32], ptr @nialang.crypto.sha256.k, i64 0, i64 %i
  %k = load i32, ptr %kptr, align 4
  %wptr2 = getelementptr [64 x i32], ptr %w, i64 0, i64 %i
  %wv = load i32, ptr %wptr2, align 4
  %t1a = add i32 %h, %bs1
  %t1b = add i32 %t1a, %ch
  %t1c = add i32 %t1b, %k
  %t1 = add i32 %t1c, %wv
  %ar2 = call i32 @nialang.crypto.rotr32(i32 %a, i32 2)
  %ar13 = call i32 @nialang.crypto.rotr32(i32 %a, i32 13)
  %ar22 = call i32 @nialang.crypto.rotr32(i32 %a, i32 22)
  %s0x = xor i32 %ar2, %ar13
  %bs0 = xor i32 %s0x, %ar22
  %ab = and i32 %a, %b
  %ac = and i32 %a, %c
  %bc = and i32 %b, %c
  %majx = xor i32 %ab, %ac
  %maj = xor i32 %majx, %bc
  %t2 = add i32 %bs0, %maj
  %new_e = add i32 %d, %t1
  %new_a = add i32 %t1, %t2
  %i_next = add i64 %i, 1
  br label %round_loop

round_exit:
  %ha = add i32 %a0, %a
  %hb = add i32 %b0, %b
  %hc = add i32 %c0, %c
  %hd = add i32 %d0, %d
  %he = add i32 %e0, %e
  %hf = add i32 %f0, %f
  %hg = add i32 %g0, %g
  %hh = add i32 %h0, %h
  store i32 %ha, ptr %s0p, align 4
  store i32 %hb, ptr %s1p, align 4
  store i32 %hc, ptr %s2p, align 4
  store i32 %hd, ptr %s3p, align 4
  store i32 %he, ptr %s4p, align 4
  store i32 %hf, ptr %s5p, align 4
  store i32 %hg, ptr %s6p, align 4
  store i32 %hh, ptr %s7p, align 4
  ret void
}

define void @nialang.crypto.sha256(ptr %data, i64 %len, ptr %out) {
entry:
  %state = alloca [8 x i32], align 4
  %st0 = getelementptr [8 x i32], ptr %state, i64 0, i64 0
  store i32 1779033703, ptr %st0, align 4
  %st1 = getelementptr [8 x i32], ptr %state, i64 0, i64 1
  store i32 -1150833019, ptr %st1, align 4
  %st2 = getelementptr [8 x i32], ptr %state, i64 0, i64 2
  store i32 1013904242, ptr %st2, align 4
  %st3 = getelementptr [8 x i32], ptr %state, i64 0, i64 3
  store i32 -1521486534, ptr %st3, align 4
  %st4 = getelementptr [8 x i32], ptr %state, i64 0, i64 4
  store i32 1359893119, ptr %st4, align 4
  %st5 = getelementptr [8 x i32], ptr %state, i64 0, i64 5
  store i32 -1694144372, ptr %st5, align 4
  %st6 = getelementptr [8 x i32], ptr %state, i64 0, i64 6
  store i32 528734635, ptr %st6, align 4
  %st7 = getelementptr [8 x i32], ptr %state, i64 0, i64 7
  store i32 1541459225, ptr %st7, align 4
  %full_blocks = udiv i64 %len, 64
  br label %full_loop

full_loop:
  %bi = phi i64 [ 0, %entry ], [ %bi_next, %full_body ]
  %full_done = icmp uge i64 %bi, %full_blocks
  br i1 %full_done, label %final_prepare, label %full_body

full_body:
  %offset = mul i64 %bi, 64
  %block_ptr = getelementptr i8, ptr %data, i64 %offset
  call void @nialang.crypto.sha256_compress(ptr %state, ptr %block_ptr)
  %bi_next = add i64 %bi, 1
  br label %full_loop

final_prepare:
  %final = alloca [128 x i8], align 1
  call void @nialang.crypto.zero_bytes(ptr %final, i64 128)
  %rem = urem i64 %len, 64
  %data_rem_off = mul i64 %full_blocks, 64
  %data_rem = getelementptr i8, ptr %data, i64 %data_rem_off
  call void @nialang.crypto.copy_bytes(ptr %final, ptr %data_rem, i64 %rem)
  %pad_ptr = getelementptr i8, ptr %final, i64 %rem
  store i8 -128, ptr %pad_ptr, align 1
  %fits_one = icmp ult i64 %rem, 56
  %len_pos = select i1 %fits_one, i64 56, i64 120
  %bit_len = shl i64 %len, 3
  %bp0s = lshr i64 %bit_len, 56
  %bp0 = trunc i64 %bp0s to i8
  %lp0 = getelementptr i8, ptr %final, i64 %len_pos
  store i8 %bp0, ptr %lp0, align 1
  %bp1s = lshr i64 %bit_len, 48
  %bp1 = trunc i64 %bp1s to i8
  %lp1i = add i64 %len_pos, 1
  %lp1 = getelementptr i8, ptr %final, i64 %lp1i
  store i8 %bp1, ptr %lp1, align 1
  %bp2s = lshr i64 %bit_len, 40
  %bp2 = trunc i64 %bp2s to i8
  %lp2i = add i64 %len_pos, 2
  %lp2 = getelementptr i8, ptr %final, i64 %lp2i
  store i8 %bp2, ptr %lp2, align 1
  %bp3s = lshr i64 %bit_len, 32
  %bp3 = trunc i64 %bp3s to i8
  %lp3i = add i64 %len_pos, 3
  %lp3 = getelementptr i8, ptr %final, i64 %lp3i
  store i8 %bp3, ptr %lp3, align 1
  %bp4s = lshr i64 %bit_len, 24
  %bp4 = trunc i64 %bp4s to i8
  %lp4i = add i64 %len_pos, 4
  %lp4 = getelementptr i8, ptr %final, i64 %lp4i
  store i8 %bp4, ptr %lp4, align 1
  %bp5s = lshr i64 %bit_len, 16
  %bp5 = trunc i64 %bp5s to i8
  %lp5i = add i64 %len_pos, 5
  %lp5 = getelementptr i8, ptr %final, i64 %lp5i
  store i8 %bp5, ptr %lp5, align 1
  %bp6s = lshr i64 %bit_len, 8
  %bp6 = trunc i64 %bp6s to i8
  %lp6i = add i64 %len_pos, 6
  %lp6 = getelementptr i8, ptr %final, i64 %lp6i
  store i8 %bp6, ptr %lp6, align 1
  %bp7 = trunc i64 %bit_len to i8
  %lp7i = add i64 %len_pos, 7
  %lp7 = getelementptr i8, ptr %final, i64 %lp7i
  store i8 %bp7, ptr %lp7, align 1
  call void @nialang.crypto.sha256_compress(ptr %state, ptr %final)
  br i1 %fits_one, label %write_digest, label %second_final

second_final:
  %second_ptr = getelementptr [128 x i8], ptr %final, i64 0, i64 64
  call void @nialang.crypto.sha256_compress(ptr %state, ptr %second_ptr)
  br label %write_digest

write_digest:
  br label %digest_loop

digest_loop:
  %di = phi i64 [ 0, %write_digest ], [ %di_next, %digest_body ]
  %di_done = icmp uge i64 %di, 8
  br i1 %di_done, label %exit, label %digest_body

digest_body:
  %sp = getelementptr [8 x i32], ptr %state, i64 0, i64 %di
  %sv = load i32, ptr %sp, align 4
  call void @nialang.crypto.store_be32(ptr %out, i64 %di, i32 %sv)
  %di_next = add i64 %di, 1
  br label %digest_loop

exit:
  ret void
}

define i1 @nialang.crypto.digest_eq(ptr %left, ptr %right) {
entry:
  br label %loop

loop:
  %i = phi i64 [ 0, %entry ], [ %next, %body ]
  %diff = phi i8 [ 0, %entry ], [ %diff_next, %body ]
  %done = icmp uge i64 %i, 32
  br i1 %done, label %exit, label %body

body:
  %lp = getelementptr i8, ptr %left, i64 %i
  %rp = getelementptr i8, ptr %right, i64 %i
  %lv = load i8, ptr %lp, align 1
  %rv = load i8, ptr %rp, align 1
  %x = xor i8 %lv, %rv
  %diff_next = or i8 %diff, %x
  %next = add i64 %i, 1
  br label %loop

exit:
  %ok = icmp eq i8 %diff, 0
  ret i1 %ok
}

define void @nialang.crypto.merkle_leaf_hash(ptr %data, i64 %len, ptr %out) {
entry:
  %total = add i64 %len, 1
  %buf = alloca i8, i64 %total, align 1
  store i8 0, ptr %buf, align 1
  %payload = getelementptr i8, ptr %buf, i64 1
  call void @nialang.crypto.copy_bytes(ptr %payload, ptr %data, i64 %len)
  call void @nialang.crypto.sha256(ptr %buf, i64 %total, ptr %out)
  ret void
}

define void @nialang.crypto.merkle_node_hash(ptr %left, ptr %right, ptr %out) {
entry:
  %buf = alloca [65 x i8], align 1
  store i8 1, ptr %buf, align 1
  %left_dst = getelementptr [65 x i8], ptr %buf, i64 0, i64 1
  call void @nialang.crypto.copy_bytes(ptr %left_dst, ptr %left, i64 32)
  %right_dst = getelementptr [65 x i8], ptr %buf, i64 0, i64 33
  call void @nialang.crypto.copy_bytes(ptr %right_dst, ptr %right, i64 32)
  call void @nialang.crypto.sha256(ptr %buf, i64 65, ptr %out)
  ret void
}

define void @nialang.crypto.merkle_root(ptr %digests, i64 %count, ptr %out) {
entry:
  br label %level_loop

level_loop:
  %cur_count = phi i64 [ %count, %entry ], [ %next_count, %after_pairs ]
  %one = icmp ule i64 %cur_count, 1
  br i1 %one, label %finish, label %pair_loop

pair_loop:
  %i = phi i64 [ 0, %level_loop ], [ %i_next, %pair_body ]
  %plus_one = add i64 %cur_count, 1
  %next_count = udiv i64 %plus_one, 2
  %pairs_done = icmp uge i64 %i, %next_count
  br i1 %pairs_done, label %after_pairs, label %pair_body

pair_body:
  %left_index = mul i64 %i, 2
  %right_index = add i64 %left_index, 1
  %has_right = icmp ult i64 %right_index, %cur_count
  %left_ptr = getelementptr [32 x i8], ptr %digests, i64 %left_index
  %right_candidate = getelementptr [32 x i8], ptr %digests, i64 %right_index
  %right_ptr = select i1 %has_right, ptr %right_candidate, ptr %left_ptr
  %out_ptr = getelementptr [32 x i8], ptr %digests, i64 %i
  call void @nialang.crypto.merkle_node_hash(ptr %left_ptr, ptr %right_ptr, ptr %out_ptr)
  %i_next = add i64 %i, 1
  br label %pair_loop

after_pairs:
  br label %level_loop

finish:
  call void @nialang.crypto.copy_bytes(ptr %out, ptr %digests, i64 32)
  ret void
}

define void @nialang.crypto.merkle_root_from_data(ptr %data, i64 %leaf_len, i64 %leaf_count, ptr %out) {
entry:
  %digests = alloca [32 x i8], i64 %leaf_count, align 1
  br label %leaf_loop

leaf_loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %leaf_body ]
  %done = icmp uge i64 %i, %leaf_count
  br i1 %done, label %root, label %leaf_body

leaf_body:
  %byte_off = mul i64 %i, %leaf_len
  %leaf_ptr = getelementptr i8, ptr %data, i64 %byte_off
  %digest_ptr = getelementptr [32 x i8], ptr %digests, i64 %i
  call void @nialang.crypto.merkle_leaf_hash(ptr %leaf_ptr, i64 %leaf_len, ptr %digest_ptr)
  %i_next = add i64 %i, 1
  br label %leaf_loop

root:
  call void @nialang.crypto.merkle_root(ptr %digests, i64 %leaf_count, ptr %out)
  ret void
}

define i1 @nialang.crypto.merkle_verify(ptr %root, ptr %leaf, i64 %index, ptr %proof, i64 %depth) {
entry:
  %acc = alloca [32 x i8], align 1
  %tmp = alloca [32 x i8], align 1
  call void @nialang.crypto.copy_bytes(ptr %acc, ptr %leaf, i64 32)
  br label %loop

loop:
  %level = phi i64 [ 0, %entry ], [ %next, %body_end ]
  %done = icmp uge i64 %level, %depth
  br i1 %done, label %finish, label %body

body:
  %sib = getelementptr [32 x i8], ptr %proof, i64 %level
  %shifted = lshr i64 %index, %level
  %bit = and i64 %shifted, 1
  %is_right = icmp eq i64 %bit, 1
  br i1 %is_right, label %right_child, label %left_child

left_child:
  call void @nialang.crypto.merkle_node_hash(ptr %acc, ptr %sib, ptr %tmp)
  br label %body_end

right_child:
  call void @nialang.crypto.merkle_node_hash(ptr %sib, ptr %acc, ptr %tmp)
  br label %body_end

body_end:
  call void @nialang.crypto.copy_bytes(ptr %acc, ptr %tmp, i64 32)
  %next = add i64 %level, 1
  br label %loop

finish:
  %ok = call i1 @nialang.crypto.digest_eq(ptr %root, ptr %acc)
  ret i1 %ok
}

"#;
