;bubble sort a static digit string

(begin
  (setq s :string "43152")
  (setq n :int (strlen s))
  (setq i :int 0)

  (loop while (< i n) do
    (setq j :int 0)

    (loop while (< j (- n 1)) do
      (if (> (strget s j) (strget s (+ j 1)))
          (let ((tmp :int (strget s j)))
            (begin
              (strset s j (strget s (+ j 1)))
              (strset s (+ j 1) tmp)))
          0)

      (setq j :int (+ j 1))
    finally 0)

    (setq i :int (+ i 1))
  finally 0)

  (print-str s)
  (halt))
