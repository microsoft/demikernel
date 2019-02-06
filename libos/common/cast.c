#include <dmtr/cast.h>

#include <dmtr/annot.h>

#include <limits.h>
#include <sys/types.h>

/* TODO: when both types are unsigned (or signed), if the size of each type
 * is identical, no conversion logic should be necessary. therefore, i
 * could use CMake to check integer sizes and optimize this. */
#define DMTR_CAST_MATCHING2(To, ToType, From, FromType, Tmp) \
    do { \
        const ToType Tmp = ((ToType)(From)); \
        if ((FromType)Tmp == (From)) { \
            *(To) = Tmp; \
            break; \
        } else { \
            *(To) = 0; \
            return ERANGE; \
        } \
    } while (0)

#define DMTR_CAST_MATCHING(To, ToType, From, FromType) \
    DMTR_CAST_MATCHING2(To, ToType, From, FromType, \
            DMTR_UNIQID(DMTR_CAST_UINTTOUINT_tmp))

#define DMTR_CAST_ITOI(To, ToType, From, FromType) \
      DMTR_CAST_MATCHING(To, ToType, From, FromType)

#define DMTR_CAST_UTOU(To, ToType, From, FromType) \
      DMTR_CAST_MATCHING(To, ToType, From, FromType)

#define DMTR_CAST_UTOI2(To, ToType, ToMax, From, Tmp) \
    do { \
        const uintmax_t Tmp = ((uintmax_t)(From)); \
        if (Tmp <= ((uintmax_t)(ToMax))) { \
            *(To) = ((ToType)(From)); \
            break; \
        } else { \
            *(To) = 0; \
            return ERANGE; \
        } \
   } while (0)

#define DMTR_CAST_UTOI(To, ToType, ToMax, From) \
    DMTR_CAST_UTOI2(To, ToType, ToMax, From, \
            DMTR_UNIQID(DMTR_CAST_UINTTOINT_tmp))

#define DMTR_CAST_ITOU2(To, ToType, ToMax, From, Tmp) \
    do { \
        if ((From) >= 0) { \
            const uintmax_t Tmp = ((uintmax_t)(From)); \
            if (Tmp <= ((uintmax_t)(ToMax))) { \
                *(To) = ((ToType)(From)); \
                break; \
            } \
        } \
        *(To) = 0; \
        return ERANGE; \
    } while (0)

#define DMTR_CAST_ITOU(To, ToType, ToMax, From) \
      DMTR_CAST_ITOU2(To, ToType, ToMax, From, \
            DMTR_UNIQID(DMTR_CAST_INTTOUINT_tmp))

int dmtr_ultoc(char *to_arg, unsigned long from_arg) {
    DMTR_NOTNULL(to_arg);

    DMTR_CAST_UTOI(to_arg, char, CHAR_MAX, from_arg);
    return 0;
}

int dmtr_sztoi32(int32_t *to_arg, size_t from_arg) {
    DMTR_NOTNULL(to_arg);

    DMTR_CAST_UTOI(to_arg, int32_t, INT32_MAX, from_arg);
    return 0;
}

int dmtr_ssztoi32(int32_t *to_arg, ssize_t from_arg) {
    DMTR_NOTNULL(to_arg);

    DMTR_CAST_ITOI(to_arg, int32_t, from_arg, ssize_t);
    return 0;
}

int dmtr_sztou32(uint32_t *to_arg, size_t from_arg) {
    DMTR_NOTNULL(to_arg);

    DMTR_CAST_UTOU(to_arg, uint32_t, from_arg, size_t);
    return 0;
}

int dmtr_sztoi(int *to_arg, size_t from_arg) {
    DMTR_NOTNULL(to_arg);

    DMTR_CAST_UTOI(to_arg, int, INT_MAX, from_arg);
    return 0;
}

int dmtr_sztoi16(short int *to_arg, size_t from_arg) {
    DMTR_NOTNULL(to_arg);

    DMTR_CAST_UTOI(to_arg, short int, INT16_MAX, from_arg);
    return 0;
}

int dmtr_ltosz(size_t *to_arg, long from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_ITOU(to_arg, size_t, SIZE_MAX, from_arg);
   return 0;
}

int dmtr_itosz(size_t *to_arg, int from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_ITOU(to_arg, size_t, SIZE_MAX, from_arg);
   return 0;
}

int ramcast_ltouc(unsigned char *to_arg, long from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_ITOU(to_arg, unsigned char, UCHAR_MAX, from_arg);
   return 0;
}

int dmtr_sztol(long *to_arg, size_t from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_UTOI(to_arg, long, LONG_MAX, from_arg);
   return 0;
}

int dmtr_sztou(unsigned int *to_arg, size_t from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_UTOU(to_arg, unsigned int, from_arg, size_t);
   return 0;
}

int dmtr_ultouc(unsigned char *to_arg,
      unsigned long from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_UTOU(to_arg, unsigned char, from_arg, unsigned long);
   return 0;
}

int dmtr_ultou(unsigned int *to_arg, unsigned long from_arg)
{
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_UTOU(to_arg, unsigned int, from_arg, unsigned long);
   return 0;
}

int dmtr_ltoc(char *to_arg, long from_arg) {
   DMTR_NOTNULL(to_arg);

   DMTR_CAST_ITOI(to_arg, char, from_arg, long);
   return 0;
}
